package com.samsung.e830

import android.os.Bundle
import android.view.View
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.lifecycleScope
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.button.MaterialButton
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong

class EmulatorActivity : AppCompatActivity() {
    private val sessionHandle = AtomicLong(0)
    private val generation = AtomicLong(0)
    private val nativeExecutor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "e830-android-native").apply { isDaemon = true }
    }

    private lateinit var root: View
    private lateinit var toolbar: MaterialToolbar
    private lateinit var lcd: LcdView
    private lateinit var waiting: TextView
    private lateinit var status: TextView
    private lateinit var actions: View
    private lateinit var fullscreenButton: MaterialButton
    private var pollingJob: Job? = null
    private var fullscreen = false

    private val claPath by lazy { intent.getStringExtra(EXTRA_CLA).orEmpty() }
    private val eepromPath by lazy { intent.getStringExtra(EXTRA_EEPROM).orEmpty() }
    private val tfsPath by lazy { intent.getStringExtra(EXTRA_TFS).orEmpty() }
    private val onenandPath by lazy { intent.getStringExtra(EXTRA_ONENAND).orEmpty() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowInsetsControllerCompat(window, window.decorView).apply {
            isAppearanceLightStatusBars = false
            isAppearanceLightNavigationBars = false
        }
        setContentView(R.layout.activity_emulator)

        root = findViewById(R.id.root)
        toolbar = findViewById(R.id.toolbar)
        lcd = findViewById(R.id.lcd_view)
        waiting = findViewById(R.id.waiting)
        status = findViewById(R.id.status)
        actions = findViewById(R.id.actions)
        fullscreenButton = findViewById(R.id.btn_fullscreen)
        applySystemInsets(root)

        fullscreenButton.setOnClickListener { setFullscreen(!fullscreen) }
        findViewById<View>(R.id.btn_back).setOnClickListener { finish() }
        lcd.setOnClickListener { if (fullscreen) setFullscreen(false) }

        if (claPath.isBlank() || eepromPath.isBlank()) {
            status.setText(R.string.choose_firmware_error)
            findViewById<View>(R.id.btn_back).post { finish() }
        }
    }

    override fun onStart() {
        super.onStart()
        if (claPath.isNotBlank() && eepromPath.isNotBlank()) startSession()
    }

    override fun onStop() {
        if (!isFinishing) status.setText(R.string.emulator_stopped_background)
        stopSession(waitForNative = true)
        super.onStop()
    }

    override fun onDestroy() {
        stopSession(waitForNative = true)
        nativeExecutor.shutdown()
        lcd.releaseFrame()
        super.onDestroy()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && fullscreen) hideSystemBars()
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

    private fun startSession() {
        val token = generation.incrementAndGet()
        status.setText(R.string.emulator_starting)
        waiting.visibility = View.VISIBLE
        nativeExecutor.execute {
            val handle = try {
                NativeBridge.nativeStart(claPath, eepromPath, tfsPath, onenandPath)
            } catch (error: Throwable) {
                runOnUiThread {
                    if (generation.get() == token) {
                        waiting.visibility = View.GONE
                        status.text = error.cause?.message ?: error.message ?: "Could not start the E830 worker."
                    }
                }
                return@execute
            }
            if (handle == 0L) return@execute
            if (generation.get() != token) {
                stopAndDestroy(handle)
                return@execute
            }
            sessionHandle.set(handle)
            runOnUiThread {
                if (generation.get() == token && sessionHandle.get() == handle) {
                    startPolling(token, handle)
                }
            }
        }
    }

    private fun startPolling(token: Long, handle: Long) {
        pollingJob?.cancel()
        pollingJob = lifecycleScope.launch {
            while (isActive && generation.get() == token && sessionHandle.get() == handle) {
                val snapshot = try {
                    withContext(Dispatchers.IO) {
                        nativeExecutor.submit<NativeSnapshot?> {
                            if (generation.get() != token || sessionHandle.get() != handle) {
                                null
                            } else {
                                val message = NativeBridge.nativePollStatus(handle)
                                val frame = NativeBridge.nativeTakeFrame(handle)
                                val running = NativeBridge.nativeIsRunning(handle)
                                NativeSnapshot(message, frame, running)
                            }
                        }.get()
                    }
                } catch (error: Throwable) {
                    if (generation.get() == token) {
                        status.text = error.cause?.message ?: error.message ?: "The E830 worker stopped unexpectedly."
                    }
                    stopSession()
                    break
                }

                if (snapshot == null || generation.get() != token || sessionHandle.get() != handle) break
                status.text = if (snapshot.status.startsWith("Running")) {
                    getString(R.string.display_status)
                } else {
                    snapshot.status
                }
                snapshot.frame?.let {
                    lcd.setFrame(it)
                    waiting.visibility = View.GONE
                }
                if (!snapshot.running) {
                    if (sessionHandle.compareAndSet(handle, 0)) {
                        nativeExecutor.execute { stopAndDestroy(handle) }
                    }
                    break
                }
                delay(50)
            }
        }
    }

    private fun stopSession(waitForNative: Boolean = false) {
        generation.incrementAndGet()
        pollingJob?.cancel()
        pollingJob = null
        val handle = sessionHandle.getAndSet(0)
        if (waitForNative) {
            try {
                nativeExecutor.submit {
                    if (handle != 0L) stopAndDestroy(handle)
                }.get()
            } catch (error: Exception) {
                android.util.Log.e("SamsungE830", "Could not stop the E830 worker cleanly.", error)
            }
        } else if (handle != 0L) {
            nativeExecutor.execute { stopAndDestroy(handle) }
        }
    }

    private fun stopAndDestroy(handle: Long) {
        try {
            NativeBridge.nativeStop(handle)
        } catch (_: Throwable) {
        } finally {
            try {
                NativeBridge.nativeDestroy(handle)
            } catch (_: Throwable) {
            }
        }
    }

    private fun setFullscreen(enabled: Boolean) {
        fullscreen = enabled
        toolbar.visibility = if (enabled) View.GONE else View.VISIBLE
        status.visibility = if (enabled) View.GONE else View.VISIBLE
        actions.visibility = if (enabled) View.GONE else View.VISIBLE
        fullscreenButton.setText(if (enabled) R.string.exit_fullscreen else R.string.fullscreen)
        fullscreenButton.setIconResource(if (enabled) R.drawable.ic_fullscreen_exit else R.drawable.ic_fullscreen)
        val controller = WindowInsetsControllerCompat(window, root)
        controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        if (enabled) {
            controller.hide(WindowInsetsCompat.Type.systemBars())
        } else {
            controller.show(WindowInsetsCompat.Type.systemBars())
        }
        ViewCompat.requestApplyInsets(root)
    }

    private fun hideSystemBars() {
        WindowInsetsControllerCompat(window, root).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }

    private data class NativeSnapshot(val status: String, val frame: ByteArray?, val running: Boolean)

    companion object {
        const val EXTRA_CLA = "com.samsung.e830.CLA"
        const val EXTRA_EEPROM = "com.samsung.e830.EEPROM"
        const val EXTRA_TFS = "com.samsung.e830.TFS"
        const val EXTRA_ONENAND = "com.samsung.e830.ONENAND"
    }
}
