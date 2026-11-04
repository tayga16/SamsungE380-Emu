package com.samsung.e830

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.view.View
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.widget.Toolbar
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.lifecycleScope
import androidx.recyclerview.widget.GridLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.floatingactionbutton.FloatingActionButton
import com.google.android.material.snackbar.Snackbar
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : AppCompatActivity() {
    private lateinit var library: FirmwareLibrary
    private lateinit var adapter: FirmwareAdapter
    private lateinit var recycler: RecyclerView
    private lateinit var emptyState: View
    private lateinit var status: TextView
    private lateinit var importButton: FloatingActionButton
    private var pendingNandEntryId: String? = null
    private var busy = false

    private val importFirmware = registerForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments(),
    ) { uris ->
        if (!uris.isNullOrEmpty()) importFirmwareFiles(uris)
    }

    private val selectOneNand = registerForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        val entryId = pendingNandEntryId
        pendingNandEntryId = null
        if (uri != null && entryId != null) attachOneNand(entryId, uri)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        pendingNandEntryId = savedInstanceState?.getString(STATE_NAND_ENTRY)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        setContentView(R.layout.activity_main)
        setSupportActionBar(findViewById<Toolbar>(R.id.toolbar))
        val root = findViewById<View>(R.id.root)
        applySystemInsets(root)
        WindowInsetsControllerCompat(window, root).isAppearanceLightStatusBars = false
        WindowInsetsControllerCompat(window, root).isAppearanceLightNavigationBars = false

        library = FirmwareLibrary(this)
        recycler = findViewById(R.id.recycler)
        emptyState = findViewById(R.id.empty_state)
        status = findViewById(R.id.status)
        importButton = findViewById(R.id.fab_import)
        adapter = FirmwareAdapter(
            onRun = ::runFirmware,
            onSelectOneNand = ::chooseOneNand,
            onUseErasedFlash = ::useAutomaticNand,
            onRemove = ::confirmRemove,
        )
        recycler.layoutManager = GridLayoutManager(this, gridSpanCount())
        recycler.adapter = adapter
        importButton.setOnClickListener {
            importFirmware.launch(arrayOf("*/*"))
        }
    }

    override fun onResume() {
        super.onResume()
        refreshLibrary()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putString(STATE_NAND_ENTRY, pendingNandEntryId)
        super.onSaveInstanceState(outState)
    }

    private fun applySystemInsets(view: View) {
        val initialLeft = view.paddingLeft
        val initialTop = view.paddingTop
        val initialRight = view.paddingRight
        val initialBottom = view.paddingBottom
        ViewCompat.setOnApplyWindowInsetsListener(view) { target, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            target.setPadding(
                initialLeft + bars.left,
                initialTop + bars.top,
                initialRight + bars.right,
                initialBottom + bars.bottom,
            )
            insets
        }
        ViewCompat.requestApplyInsets(view)
    }

    private fun gridSpanCount(): Int = (resources.configuration.screenWidthDp / 150).coerceAtLeast(2)

    private fun refreshLibrary() {
        lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { library.entries() } }
            result.fold(
                onSuccess = { entries ->
                    adapter.submit(entries)
                    recycler.visibility = if (entries.isEmpty()) View.GONE else View.VISIBLE
                    emptyState.visibility = if (entries.isEmpty()) View.VISIBLE else View.GONE
                },
                onFailure = { error ->
                    recycler.visibility = View.GONE
                    emptyState.visibility = View.VISIBLE
                    status.text = getString(R.string.library_error, error.message ?: "unknown error")
                },
            )
        }
    }

    private fun importFirmwareFiles(uris: List<Uri>) {
        if (busy) return
        setBusy(true)
        status.setText(R.string.import_in_progress)
        lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching { library.importFirmware(uris) }
            }
            setBusy(false)
            result.fold(
                onSuccess = { entry ->
                    status.text = getString(R.string.import_success, entry.name)
                    refreshLibrary()
                },
                onFailure = { error ->
                    status.text = getString(R.string.source_files_untouched)
                    showMessage(getString(R.string.import_failed, error.message ?: "unknown error"))
                },
            )
        }
    }

    private fun chooseOneNand(entry: FirmwareEntry) {
        if (busy) return
        pendingNandEntryId = entry.id
        selectOneNand.launch(arrayOf("*/*"))
    }

    private fun attachOneNand(entryId: String, uri: Uri) {
        if (busy) return
        setBusy(true)
        lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    val entry = library.entries().firstOrNull { it.id == entryId }
                        ?: throw IllegalStateException("This firmware entry no longer exists.")
                    library.attachOneNand(entry, uri)
                }
            }
            setBusy(false)
            result.fold(
                onSuccess = {
                    status.setText(R.string.nand_present)
                    refreshLibrary()
                },
                onFailure = { error -> showMessage(error.message ?: "Could not use this NAND image.") },
            )
        }
    }

    private fun useAutomaticNand(entry: FirmwareEntry) {
        if (busy) return
        setBusy(true)
        lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching { library.useAutomaticNand(entry) }
            }
            setBusy(false)
            result.fold(
                onSuccess = {
                    status.setText(R.string.nand_missing)
                    refreshLibrary()
                },
                onFailure = { error -> showMessage(error.message ?: "Could not remove the imported NAND copy.") },
            )
        }
    }

    private fun runFirmware(entry: FirmwareEntry) {
        if (busy) return
        startActivity(
            Intent(this, EmulatorActivity::class.java)
                .putExtra(EmulatorActivity.EXTRA_CLA, entry.cla.absolutePath)
                .putExtra(EmulatorActivity.EXTRA_EEPROM, entry.eeprom.absolutePath)
                .putExtra(EmulatorActivity.EXTRA_TFS, entry.tfs?.absolutePath.orEmpty())
                .putExtra(EmulatorActivity.EXTRA_ONENAND, entry.onenand?.absolutePath.orEmpty()),
        )
    }

    private fun confirmRemove(entry: FirmwareEntry) {
        if (busy) return
        com.google.android.material.dialog.MaterialAlertDialogBuilder(this)
            .setTitle(R.string.remove_title)
            .setMessage(getString(R.string.remove_message, entry.name))
            .setNegativeButton(R.string.cancel, null)
            .setPositiveButton(R.string.remove) { _, _ -> removeFirmware(entry) }
            .show()
    }

    private fun removeFirmware(entry: FirmwareEntry) {
        if (busy) return
        setBusy(true)
        lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { library.remove(entry) } }
            setBusy(false)
            result.fold(
                onSuccess = {
                    status.setText(R.string.source_files_untouched)
                    refreshLibrary()
                },
                onFailure = { error -> showMessage(error.message ?: "Could not remove this firmware set.") },
            )
        }
    }

    private fun setBusy(busy: Boolean) {
        this.busy = busy
        importButton.isEnabled = !busy
        recycler.isEnabled = !busy
        recycler.alpha = if (busy) 0.55f else 1f
    }

    private fun showMessage(message: String) {
        Snackbar.make(findViewById(R.id.root), message, Snackbar.LENGTH_LONG).show()
    }

    private companion object {
        const val STATE_NAND_ENTRY = "pendingNandEntryId"
    }
}
