package com.samsung.e830

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.util.AttributeSet
import android.view.View
import kotlin.math.min

internal class LcdView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : View(context, attrs, defStyleAttr) {
    private var frame: Bitmap? = null
    private val paint = Paint().apply {
        isAntiAlias = false
        isFilterBitmap = false
    }

    fun setFrame(rgb: ByteArray) {
        val width = 176
        val height = 220
        if (rgb.size != width * height * 3) return
        val colors = IntArray(width * height)
        var source = 0
        for (index in colors.indices) {
            val red = rgb[source++].toInt() and 0xff
            val green = rgb[source++].toInt() and 0xff
            val blue = rgb[source++].toInt() and 0xff
            colors[index] = Color.rgb(red, green, blue)
        }
        val next = Bitmap.createBitmap(colors, width, height, Bitmap.Config.ARGB_8888)
        val previous = frame
        frame = next
        previous?.recycle()
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.drawColor(Color.BLACK)
        val bitmap = frame ?: return
        if (width <= 0 || height <= 0) return
        val scale = min(2f, min(width.toFloat() / bitmap.width, height.toFloat() / bitmap.height))
            .coerceAtLeast(0.1f)
        val displayWidth = bitmap.width * scale
        val displayHeight = bitmap.height * scale
        val left = (width - displayWidth) / 2f
        val top = (height - displayHeight) / 2f
        canvas.drawBitmap(bitmap, null, RectF(left, top, left + displayWidth, top + displayHeight), paint)
    }

    fun releaseFrame() {
        frame?.recycle()
        frame = null
        invalidate()
    }
}
