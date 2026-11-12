package dev.snova.novaterm

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.util.TypedValue
import android.view.GestureDetector
import android.view.KeyCharacterMap
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.widget.PopupMenu
import android.widget.Toast
import java.io.File
import kotlin.math.max

/** Draws the terminal grid and turns touch/keyboard input into bytes for the shell. */
class TerminalView(context: Context, private val settings: Settings) : View(context) {

    /** Sticky modifiers toggled from the extra-keys bar; cleared after one key. */
    var ctrlSticky = false
    var altSticky = false
    var onStickyChanged: (() -> Unit)? = null
    var onOpenSettings: (() -> Unit)? = null

    private var handle = 0L
    private var exited = false
    @Volatile private var destroyed = false
    private var startError: String? = null

    private var rows = 0
    private var cols = 0
    private var scroll = 0          // lines scrolled back into history
    private var scrollbackLen = 0
    private var flags = 0           // bit0 app-cursor, bit1 bracketed paste, bit2 alt screen
    private var lastSnap: IntArray? = null

    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { typeface = Typeface.MONOSPACE }
    private val rectPaint = Paint()
    private var fontPx = 0f
    private var cellW = 1f
    private var cellH = 1
    private var baseline = 0
    private val runBuf = CharArray(1024)

    private val main = Handler(Looper.getMainLooper())
    private var blinkOn = true
    private val blinker = object : Runnable {
        override fun run() {
            blinkOn = !blinkOn
            invalidate()
            main.postDelayed(this, 530)
        }
    }

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        applySettings()
    }

    // ───────────────────────── settings ─────────────────────────

    private fun pxPerSp(): Float =
        TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, 1f, resources.displayMetrics)

    /** Re-read preferences: font, colors, cursor, screen-on. */
    fun applySettings() {
        setFontPx(settings.fontSp * pxPerSp())
        if (handle != 0L) Native.setTheme(handle, settings.themeInts())
        keepScreenOn = settings.keepScreenOn
        restartBlink()
        invalidate()
    }

    private fun setFontPx(px: Float) {
        fontPx = px
        paint.textSize = px
        cellW = max(1f, paint.measureText("M"))
        val fm = paint.fontMetricsInt
        cellH = max(1, fm.descent - fm.ascent)
        baseline = -fm.ascent
        layoutGrid()
    }

    private fun restartBlink() {
        main.removeCallbacks(blinker)
        blinkOn = true
        if (settings.cursorBlink && isAttachedToWindow) main.postDelayed(blinker, 530)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        restartBlink()
    }

    override fun onDetachedFromWindow() {
        main.removeCallbacks(blinker)
        super.onDetachedFromWindow()
    }

    // ───────────────────────── session lifecycle ─────────────────────────

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        layoutGrid()
    }

    private fun layoutGrid() {
        if (width == 0 || height == 0 || destroyed) return
        val c = max(2, (width / cellW).toInt())
        val r = max(1, height / cellH)
        if (handle == 0L) {
            rows = r
            cols = c
            start()
        } else if (r != rows || c != cols) {
            rows = r
            cols = c
            Native.resize(handle, r, c)
        }
        invalidate()
    }

    private fun start() {
        val home = context.filesDir.apply { mkdirs() }
        var shell = settings.shell
        if (!File(shell).canExecute()) shell = Settings.DEFAULT_SHELL

        val env = HashMap(System.getenv())
        env["HOME"] = home.path
        env["TMPDIR"] = context.cacheDir.path
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        env["LANG"] = "en_US.UTF-8"
        env["SHELL"] = shell
        if (env["PATH"].isNullOrEmpty()) env["PATH"] = "/product/bin:/apex/com.android.runtime/bin:/system/bin:/system/xbin"
        val envArray = env.map { "${it.key}=${it.value}" }.toTypedArray()

        val h = Native.create(shell, arrayOf(shell), envArray, home.path, rows, cols)
        if (h == 0L) {
            startError = "Could not start $shell"
            return
        }
        startError = null
        handle = h
        exited = false
        scroll = 0
        Native.setTheme(h, settings.themeInts())
        Native.feedLocal(
            h,
            "\u001b[2mNovaTerm · Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}) · $shell\u001b[0m\r\n".toByteArray()
        )

        Thread({
            while (true) {
                val r = Native.waitUpdate(h)
                if (r >= 0) {
                    main.post { onProcessExit(h, r) }
                    break
                }
                postInvalidate()
            }
        }, "novaterm-ui-wait").start()
    }

    private fun onProcessExit(h: Long, code: Int) {
        if (destroyed || h != handle) {
            Native.free(h)
            return
        }
        exited = true
        Native.feedLocal(
            h,
            "\r\n\u001b[2m[process exited with code $code — press Enter for a new shell]\u001b[0m".toByteArray()
        )
        invalidate()
    }

    private fun restart() {
        val old = handle
        handle = 0L
        exited = false
        lastSnap = null
        if (old != 0L) Native.free(old)
        layoutGrid()
    }

    /** Call from Activity.onDestroy: ends the shell and releases native memory. */
    fun destroy() {
        destroyed = true
        main.removeCallbacks(blinker)
        val h = handle
        handle = 0L
        if (h == 0L) return
        if (exited) Native.free(h) else Native.hangup(h) // freed in onProcessExit once it dies
    }

    // ───────────────────────── drawing ─────────────────────────

    override fun onDraw(canvas: Canvas) {
        val bgDefault = settings.bg or 0xFF000000.toInt()
        if (handle == 0L) {
            canvas.drawColor(bgDefault)
            startError?.let {
                paint.color = settings.fg or 0xFF000000.toInt()
                paint.isFakeBoldText = false
                canvas.drawText(it, cellW, (baseline + cellH).toFloat(), paint)
            }
            return
        }
        val s = Native.snapshot(handle, scroll)
        if (s.size < HEADER) return
        lastSnap = s
        val r = s[0]
        val c = s[1]
        val curRow = s[2]
        val curCol = s[3]
        scrollbackLen = s[4]
        flags = s[5]
        val bg0 = s[6]
        if (scroll > scrollbackLen) scroll = scrollbackLen
        canvas.drawColor(bg0)

        for (y in 0 until r) {
            val top = (y * cellH).toFloat()
            var x = 0
            while (x < c) {
                val i = HEADER + (y * c + x) * 4
                val cp = s[i]
                val fg = s[i + 1]
                val bg = s[i + 2]
                val attr = s[i + 3]
                var end = x + 1
                if (cp in 32..126) {
                    // batch plain ASCII with identical style into one drawText call
                    while (end < c && end - x < runBuf.size) {
                        val j = HEADER + (y * c + end) * 4
                        if (s[j] !in 32..126 || s[j + 1] != fg || s[j + 2] != bg || s[j + 3] != attr) break
                        end++
                    }
                } else if (cp != 0 && end < c && s[HEADER + (y * c + end) * 4] == 0) {
                    end++ // wide glyph spans two cells
                }
                val left = x * cellW
                if (bg != bg0) {
                    rectPaint.color = bg
                    canvas.drawRect(left, top, end * cellW, top + cellH, rectPaint)
                }
                if (cp > 32 || (cp == 32 && (end - x > 1 || attr and (ATTR_UNDERLINE or ATTR_STRIKE) != 0))) {
                    stylePaint(fg, attr)
                    if (cp in 32..126) {
                        for (k in x until end) runBuf[k - x] = s[HEADER + (y * c + k) * 4].toChar()
                        canvas.drawText(runBuf, 0, end - x, left, top + baseline, paint)
                    } else {
                        val n = Character.toChars(cp, runBuf, 0)
                        canvas.drawText(runBuf, 0, n, left, top + baseline, paint)
                    }
                }
                x = end
            }
        }

        if (curRow in 0 until r && curCol in 0 until c) drawCursor(canvas, s, c, curRow, curCol)
    }

    private fun stylePaint(color: Int, attr: Int) {
        paint.color = color
        paint.isFakeBoldText = attr and ATTR_BOLD != 0
        paint.textSkewX = if (attr and ATTR_ITALIC != 0) -0.2f else 0f
        paint.isUnderlineText = attr and ATTR_UNDERLINE != 0
        paint.isStrikeThruText = attr and ATTR_STRIKE != 0
    }

    private fun drawCursor(canvas: Canvas, s: IntArray, c: Int, row: Int, col: Int) {
        val left = col * cellW
        val top = (row * cellH).toFloat()
        val right = left + cellW
        val bottom = top + cellH
        rectPaint.color = s[7]
        if (!hasFocus()) {
            rectPaint.style = Paint.Style.STROKE
            rectPaint.strokeWidth = max(2f, cellW / 10)
            canvas.drawRect(left + 1, top + 1, right - 1, bottom - 1, rectPaint)
            rectPaint.style = Paint.Style.FILL
            return
        }
        if (settings.cursorBlink && !blinkOn) return
        when (settings.cursorStyle) {
            1 -> canvas.drawRect(left, bottom - max(2f, cellH / 8f), right, bottom, rectPaint)
            2 -> canvas.drawRect(left, top, left + max(2f, cellW / 6), bottom, rectPaint)
            else -> {
                canvas.drawRect(left, top, right, bottom, rectPaint)
                val i = HEADER + (row * c + col) * 4
                val cp = s[i]
                if (cp > 32) {
                    stylePaint(s[6], s[i + 3] and ATTR_BOLD)
                    val n = Character.toChars(cp, runBuf, 0)
                    canvas.drawText(runBuf, 0, n, left, top + baseline, paint)
                }
            }
        }
    }

    // ───────────────────────── output to the shell ─────────────────────────

    private fun write(text: String) {
        if (handle == 0L || exited || text.isEmpty()) return
        scroll = 0
        blinkOn = true
        Native.write(handle, text.toByteArray(Charsets.UTF_8))
        invalidate()
    }

    private fun clearSticky() {
        if (ctrlSticky || altSticky) {
            ctrlSticky = false
            altSticky = false
            onStickyChanged?.invoke()
        }
    }

    /** Text typed on the soft keyboard or tapped on the extra-keys bar. */
    fun sendText(text: String) {
        if (text.isEmpty()) return
        if (exited) {
            if (text == "\n" || text == "\r") restart()
            return
        }
        if ((ctrlSticky || altSticky) && text.codePointCount(0, text.length) == 1) {
            sendCodePoint(text.codePointAt(0), false, false)
        } else {
            write(text.replace("\r\n", "\r").replace('\n', '\r'))
        }
    }

    private fun sendCodePoint(cp: Int, ctrlHw: Boolean, altHw: Boolean) {
        val ctrl = ctrlHw || ctrlSticky
        val alt = altHw || altSticky
        var out = String(Character.toChars(cp))
        if (ctrl) {
            val code = ctrlCode(cp)
            if (code >= 0) out = code.toChar().toString()
        }
        if (alt) out = "\u001b$out"
        write(out)
        clearSticky()
    }

    private fun ctrlCode(cp: Int): Int = when (cp) {
        in 'a'.code..'z'.code -> cp - 96
        in 'A'.code..'Z'.code -> cp - 64
        ' '.code, '@'.code, '2'.code -> 0
        '['.code -> 27
        '\\'.code -> 28
        ']'.code -> 29
        '^'.code, '6'.code -> 30
        '_'.code, '-'.code, '/'.code -> 31
        '?'.code -> 127
        else -> -1
    }

    /** Special (non-printing) keys. Returns false if the key code isn't one of them. */
    fun sendKey(keyCode: Int, shift: Boolean = false, ctrlHw: Boolean = false, altHw: Boolean = false): Boolean {
        val ctrl = ctrlHw || ctrlSticky
        val alt = altHw || altSticky
        val seq = keySequence(keyCode, shift, ctrl) ?: return false
        if (exited) {
            if (keyCode == KeyEvent.KEYCODE_ENTER || keyCode == KeyEvent.KEYCODE_NUMPAD_ENTER) restart()
            return true
        }
        write(if (alt) "\u001b$seq" else seq)
        clearSticky()
        return true
    }

    private fun keySequence(keyCode: Int, shift: Boolean, ctrl: Boolean): String? {
        val app = flags and 1 != 0
        fun cursorKey(c: Char) = when {
            ctrl -> "\u001b[1;5$c"
            shift -> "\u001b[1;2$c"
            app -> "\u001bO$c"
            else -> "\u001b[$c"
        }
        return when (keyCode) {
            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> "\r"
            KeyEvent.KEYCODE_DEL -> if (ctrl) "\u0008" else "\u007f"
            KeyEvent.KEYCODE_TAB -> if (shift) "\u001b[Z" else "\t"
            KeyEvent.KEYCODE_ESCAPE -> "\u001b"
            KeyEvent.KEYCODE_DPAD_UP -> cursorKey('A')
            KeyEvent.KEYCODE_DPAD_DOWN -> cursorKey('B')
            KeyEvent.KEYCODE_DPAD_RIGHT -> cursorKey('C')
            KeyEvent.KEYCODE_DPAD_LEFT -> cursorKey('D')
            KeyEvent.KEYCODE_MOVE_HOME -> cursorKey('H')
            KeyEvent.KEYCODE_MOVE_END -> cursorKey('F')
            KeyEvent.KEYCODE_INSERT -> "\u001b[2~"
            KeyEvent.KEYCODE_FORWARD_DEL -> "\u001b[3~"
            KeyEvent.KEYCODE_PAGE_UP -> "\u001b[5~"
            KeyEvent.KEYCODE_PAGE_DOWN -> "\u001b[6~"
            KeyEvent.KEYCODE_F1 -> "\u001bOP"
            KeyEvent.KEYCODE_F2 -> "\u001bOQ"
            KeyEvent.KEYCODE_F3 -> "\u001bOR"
            KeyEvent.KEYCODE_F4 -> "\u001bOS"
            KeyEvent.KEYCODE_F5 -> "\u001b[15~"
            KeyEvent.KEYCODE_F6 -> "\u001b[17~"
            KeyEvent.KEYCODE_F7 -> "\u001b[18~"
            KeyEvent.KEYCODE_F8 -> "\u001b[19~"
            KeyEvent.KEYCODE_F9 -> "\u001b[20~"
            KeyEvent.KEYCODE_F10 -> "\u001b[21~"
            KeyEvent.KEYCODE_F11 -> "\u001b[23~"
            KeyEvent.KEYCODE_F12 -> "\u001b[24~"
            else -> null
        }
    }

    // ───────────────────────── hardware / IME keys ─────────────────────────

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (event.isSystem) return super.onKeyDown(keyCode, event)
        if (sendKey(keyCode, event.isShiftPressed, event.isCtrlPressed, event.isAltPressed)) return true
        val meta = event.metaState and (KeyEvent.META_CTRL_MASK or KeyEvent.META_ALT_MASK).inv()
        val uc = event.getUnicodeChar(meta)
        if (uc == 0 || uc and KeyCharacterMap.COMBINING_ACCENT != 0) return super.onKeyDown(keyCode, event)
        if (exited) return true
        sendCodePoint(uc, event.isCtrlPressed, event.isAltPressed)
        return true
    }

    override fun onCheckIsTextEditor(): Boolean = true

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        // "Visible password" makes keyboards send raw characters: no autocorrect, no suggestions.
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_FLAG_NO_EXTRACT_UI or
            EditorInfo.IME_ACTION_NONE

        return object : BaseInputConnection(this, false) {
            private var composing = ""

            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                composing = ""
                sendText(text?.toString() ?: "")
                return true
            }

            override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
                composing = text?.toString() ?: ""
                return true
            }

            override fun finishComposingText(): Boolean {
                if (composing.isNotEmpty()) {
                    val t = composing
                    composing = ""
                    sendText(t)
                }
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                if (composing.isNotEmpty()) {
                    composing = composing.dropLast(max(1, beforeLength))
                    return true
                }
                repeat(max(1, beforeLength)) { sendKey(KeyEvent.KEYCODE_DEL) }
                return true
            }
        }
    }

    fun showKeyboard() {
        requestFocus()
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.showSoftInput(this, 0)
    }

    // ───────────────────────── touch ─────────────────────────

    private var scrollAccum = 0f

    private val gestures = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
        override fun onDown(e: MotionEvent): Boolean {
            scrollAccum = 0f
            return true
        }

        override fun onSingleTapUp(e: MotionEvent): Boolean {
            showKeyboard()
            return true
        }

        override fun onScroll(e1: MotionEvent?, e2: MotionEvent, distanceX: Float, distanceY: Float): Boolean {
            scrollAccum += distanceY
            val lines = (scrollAccum / cellH).toInt()
            if (lines == 0) return true
            scrollAccum -= lines * cellH
            if (flags and 4 != 0) {
                // full-screen apps (vim, less, htop): translate to arrow keys
                val key = if (lines > 0) KeyEvent.KEYCODE_DPAD_DOWN else KeyEvent.KEYCODE_DPAD_UP
                repeat(kotlin.math.abs(lines).coerceAtMost(20)) { sendKey(key) }
            } else {
                scroll = (scroll - lines).coerceIn(0, scrollbackLen)
                invalidate()
            }
            return true
        }

        override fun onLongPress(e: MotionEvent) {
            showMenu()
        }
    })

    private val pinch = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(d: ScaleGestureDetector): Boolean {
            val sp = pxPerSp()
            setFontPx((fontPx * d.scaleFactor).coerceIn(Settings.MIN_FONT * sp, Settings.MAX_FONT * sp))
            return true
        }

        override fun onScaleEnd(d: ScaleGestureDetector) {
            settings.fontSp = fontPx / pxPerSp()
        }
    })

    override fun onTouchEvent(event: MotionEvent): Boolean {
        pinch.onTouchEvent(event)
        if (!pinch.isInProgress) gestures.onTouchEvent(event)
        return true
    }

    private fun showMenu() {
        val menu = PopupMenu(context, this)
        menu.menu.add(0, 1, 0, "Copy screen")
        menu.menu.add(0, 2, 1, "Paste")
        menu.menu.add(0, 3, 2, "Settings")
        menu.setOnMenuItemClickListener {
            when (it.itemId) {
                1 -> copyScreen()
                2 -> paste()
                3 -> onOpenSettings?.invoke()
            }
            true
        }
        menu.show()
    }

    private fun copyScreen() {
        val s = lastSnap ?: return
        val r = s[0]
        val c = s[1]
        val sb = StringBuilder()
        for (y in 0 until r) {
            val line = StringBuilder()
            for (x in 0 until c) {
                val cp = s[HEADER + (y * c + x) * 4]
                if (cp != 0) line.appendCodePoint(cp)
            }
            sb.append(line.trimEnd()).append('\n')
        }
        val clip = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clip.setPrimaryClip(ClipData.newPlainText("NovaTerm", sb.toString().trimEnd('\n')))
        Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
    }

    private fun paste() {
        val clip = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val item = clip.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0) ?: return
        val text = item.coerceToText(context)?.toString() ?: return
        if (text.isEmpty()) return
        val body = text.replace("\r\n", "\r").replace('\n', '\r').replace("\u001b", "")
        write(if (flags and 2 != 0) "\u001b[200~$body\u001b[201~" else body)
    }

    private companion object {
        const val HEADER = 8
        const val ATTR_BOLD = 1
        const val ATTR_ITALIC = 4
        const val ATTR_UNDERLINE = 8
        const val ATTR_STRIKE = 128
    }
}
