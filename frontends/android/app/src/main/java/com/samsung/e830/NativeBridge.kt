package com.samsung.e830

internal object NativeBridge {
    init {
        System.loadLibrary("e830_android_jni")
    }

    external fun nativeValidate(cla: String, eeprom: String, tfs: String, onenand: String): String
    external fun nativeStart(cla: String, eeprom: String, tfs: String, onenand: String): Long
    external fun nativePollStatus(handle: Long): String
    external fun nativeTakeFrame(handle: Long): ByteArray?
    external fun nativeIsRunning(handle: Long): Boolean
    external fun nativeStop(handle: Long)
    external fun nativeDestroy(handle: Long)
}
