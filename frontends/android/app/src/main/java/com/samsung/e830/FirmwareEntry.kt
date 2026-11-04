package com.samsung.e830

import java.io.File

internal data class FirmwareEntry(
    val id: String,
    val name: String,
    val directory: File,
    val cla: File,
    val eeprom: File,
    val tfs: File?,
    val onenand: File?,
)
