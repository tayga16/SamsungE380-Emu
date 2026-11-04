package com.samsung.e830

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.util.AtomicFile
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.util.Locale
import java.util.UUID

internal class FirmwareLibrary(context: Context) {
    private val resolver: ContentResolver = context.applicationContext.contentResolver
    private val root = File(context.applicationContext.filesDir, "firmware-library")
    private val indexFile = File(root, "library.json")
    private val index = AtomicFile(indexFile)
    private var recovered = false

    init {
        if (!root.exists() && !root.mkdirs()) throw IOException("Could not create the firmware library.")
    }

    @Synchronized
    fun entries(): List<FirmwareEntry> {
        ensureRecovered()
        return readRecords().mapNotNull { record ->
            val directory = File(root, record.id)
            val cla = File(directory, "firmware.cla")
            val eeprom = File(directory, "eeprom.nsf")
            if (!directory.isDirectory || !cla.isFile || !eeprom.isFile) return@mapNotNull null
            val tfs = File(directory, "filesystem.tfs").takeIf(File::isFile)
            val onenand = File(directory, "physical.onenand.bin").takeIf(File::isFile)
            FirmwareEntry(record.id, record.name, directory, cla, eeprom, tfs, onenand)
        }.sortedBy { it.name.lowercase(Locale.ROOT) }
    }

    @Synchronized
    fun importFirmware(uris: List<Uri>): FirmwareEntry {
        ensureRecovered()
        val selected = linkedMapOf<String, Pair<Uri, String>>()
        for (uri in uris) {
            val name = displayName(uri)
            val extension = name.substringAfterLast('.', "").lowercase(Locale.ROOT)
            if (extension !in setOf("cla", "nsf", "tfs")) {
                throw IllegalArgumentException("Select only .cla firmware, .nsf EEPROM and optional .tfs files.")
            }
            if (selected.putIfAbsent(extension, uri to name) != null) {
                throw IllegalArgumentException("Select only one file of each E830 firmware type.")
            }
        }
        val cla = selected["cla"] ?: throw IllegalArgumentException("The .cla firmware file was not selected.")
        val eeprom = selected["nsf"] ?: throw IllegalArgumentException("The .nsf EEPROM file was not selected.")
        val tfs = selected["tfs"]
        val id = UUID.randomUUID().toString()
        val staging = File(root, ".import-$id")
        val destination = File(root, id)
        if (!staging.mkdirs()) throw IOException("Could not stage the firmware import.")
        try {
            val claFile = copyUri(cla.first, File(staging, "firmware.cla"))
            val eepromFile = copyUri(eeprom.first, File(staging, "eeprom.nsf"))
            val tfsFile = tfs?.let { copyUri(it.first, File(staging, "filesystem.tfs")) }
            val error = NativeBridge.nativeValidate(
                claFile.absolutePath,
                eepromFile.absolutePath,
                tfsFile?.absolutePath.orEmpty(),
                "",
            )
            if (error.isNotBlank()) throw IllegalArgumentException(error)
            if (!staging.renameTo(destination)) throw IOException("Could not finish the firmware import.")
            val display = cla.second.substringBeforeLast('.', cla.second).ifBlank { "Samsung SGH-E830" }.take(80)
            writeRecords(readRecords() + Record(id, display))
            return FirmwareEntry(
                id,
                display,
                destination,
                File(destination, "firmware.cla"),
                File(destination, "eeprom.nsf"),
                tfs?.let { File(destination, "filesystem.tfs") },
                null,
            )
        } catch (error: Throwable) {
            staging.deleteRecursively()
            destination.deleteRecursively()
            throw error
        }
    }

    @Synchronized
    fun attachOneNand(entry: FirmwareEntry, uri: Uri): FirmwareEntry {
        ensureRecovered()
        val name = displayName(uri)
        val staging = File(entry.directory, ".physical-onenand.tmp")
        val destination = File(entry.directory, "physical.onenand.bin")
        if (staging.exists() && !staging.delete()) throw IOException("Could not clear a previous NAND import attempt.")
        try {
            copyUri(uri, staging)
            val error = NativeBridge.nativeValidate(
                entry.cla.absolutePath,
                entry.eeprom.absolutePath,
                entry.tfs?.absolutePath.orEmpty(),
                staging.absolutePath,
            )
            if (error.isNotBlank()) throw IllegalArgumentException(error)
            val atomic = AtomicFile(destination)
            val output = atomic.startWrite()
            try {
                staging.inputStream().use { source -> source.copyTo(output) }
                atomic.finishWrite(output)
            } catch (error: Throwable) {
                atomic.failWrite(output)
                throw error
            }
            staging.delete()
            return entry.copy(onenand = destination)
        } catch (error: Throwable) {
            staging.delete()
            throw IllegalArgumentException("Could not use $name: ${error.message ?: "invalid file"}", error)
        }
    }

    @Synchronized
    fun useAutomaticNand(entry: FirmwareEntry): FirmwareEntry {
        ensureRecovered()
        val stored = File(entry.directory, "physical.onenand.bin")
        val relatedFiles = listOf(
            stored,
            File(entry.directory, "physical.onenand.bin.bak"),
            File(entry.directory, "physical.onenand.bin.new"),
            File(entry.directory, ".physical-onenand.tmp"),
        )
        AtomicFile(stored).delete()
        if (relatedFiles.any(File::exists)) throw IOException("Could not remove the imported NAND copy.")
        return entry.copy(onenand = null)
    }

    @Synchronized
    fun remove(entry: FirmwareEntry) {
        ensureRecovered()
        val remaining = readRecords().filterNot { it.id == entry.id }
        val tombstone = File(root, ".remove-${entry.id}")
        if (tombstone.exists()) throw IOException("A previous firmware removal is still being recovered.")
        if (!entry.directory.renameTo(tombstone)) throw IOException("Could not stage this firmware entry for removal.")
        try {
            writeRecords(remaining)
        } catch (error: Throwable) {
            if (!tombstone.renameTo(entry.directory)) {
                error.addSuppressed(IOException("Could not restore the firmware entry after the removal failed."))
            }
            throw error
        }
        tombstone.deleteRecursively()
    }

    private fun ensureRecovered() {
        if (!recovered) {
            recoverPendingChanges()
            recovered = true
        }
    }

    private fun recoverPendingChanges() {
        val recordIds = readRecords().mapTo(mutableSetOf()) { it.id }
        val pendingFiles = root.listFiles() ?: return
        for (pending in pendingFiles) {
            when {
                pending.name.startsWith(".import-") -> pending.deleteRecursively()
                pending.name.startsWith(".remove-") -> {
                    val id = pending.name.removePrefix(".remove-")
                    if (ID_PATTERN.matches(id) && id in recordIds && !File(root, id).exists()) {
                        if (!pending.renameTo(File(root, id))) {
                            throw IOException("Could not restore a firmware entry after an interrupted removal.")
                        }
                    } else {
                        pending.deleteRecursively()
                    }
                }
                ID_PATTERN.matches(pending.name) && pending.isDirectory -> {
                    if (pending.name !in recordIds) {
                        pending.deleteRecursively()
                    } else {
                        recoverOneNand(pending)
                        File(pending, ".physical-onenand.tmp").delete()
                    }
                }
            }
        }
    }

    private fun recoverOneNand(directory: File) {
        val nand = File(directory, "physical.onenand.bin")
        val backup = File(directory, "physical.onenand.bin.bak")
        val temporary = File(directory, "physical.onenand.bin.new")
        if (nand.exists() || backup.exists()) AtomicFile(nand).openRead().use { }
        temporary.delete()
    }

    private fun displayName(uri: Uri): String {
        return resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (!cursor.moveToFirst()) null else cursor.getString(0)
        }?.takeIf(String::isNotBlank) ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf(String::isNotBlank)
        ?: throw IOException("The selected document has no file name.")
    }

    private fun copyUri(uri: Uri, destination: File): File {
        val input = resolver.openInputStream(uri) ?: throw IOException("Could not open the selected file.")
        input.use { source ->
            destination.outputStream().use { output ->
                val buffer = ByteArray(64 * 1024)
                var total = 0L
                while (true) {
                    val count = source.read(buffer)
                    if (count < 0) break
                    total += count
                    if (total > MAX_FILE_BYTES) throw IOException("A selected file exceeds the 128 MiB import limit.")
                    output.write(buffer, 0, count)
                }
                if (total == 0L) throw IOException("A selected file is empty.")
            }
        }
        return destination
    }

    private fun readRecords(): List<Record> {
        val backup = File(root, "library.json.bak")
        val temporary = File(root, "library.json.new")
        if (!indexFile.exists() && !backup.exists()) {
            temporary.delete()
            return emptyList()
        }
        val input = index.openRead()
        temporary.delete()
        return input.use { stream ->
            val array = JSONArray(stream.bufferedReader().readText())
            buildList {
                for (position in 0 until array.length()) {
                    val item = array.optJSONObject(position) ?: continue
                    val id = item.optString("id")
                    val name = item.optString("name")
                    if (ID_PATTERN.matches(id) && name.isNotBlank()) add(Record(id, name.take(80)))
                }
            }
        }
    }

    private fun writeRecords(records: List<Record>) {
        val array = JSONArray()
        records.forEach { record ->
            array.put(JSONObject().put("id", record.id).put("name", record.name))
        }
        val output = index.startWrite()
        try {
            output.write(array.toString(2).toByteArray(Charsets.UTF_8))
            index.finishWrite(output)
        } catch (error: Throwable) {
            index.failWrite(output)
            throw error
        }
    }

    private data class Record(val id: String, val name: String)

    private companion object {
        const val MAX_FILE_BYTES = 128L * 1024L * 1024L
        val ID_PATTERN = Regex("^[0-9a-fA-F-]{36}$")
    }
}
