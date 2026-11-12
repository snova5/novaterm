package dev.snova.novaterm

import android.app.Activity
import android.graphics.Typeface
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView

class MainActivity : Activity() {

    private lateinit var settings: Settings
    private lateinit var root: LinearLayout
    private lateinit var term: TerminalView
    private lateinit var keyBar: HorizontalScrollView
    private lateinit var ctrlKey: TextView
    private lateinit var altKey: TextView
    private lateinit var settingsKey: TextView
    private val keyViews = ArrayList<TextView>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        settings = Settings(this)

        // Edge-to-edge is mandatory from Android 16 (targetSdk 36): we draw behind
        // the system bars and pad the content ourselves, including for the keyboard.
        window.setDecorFitsSystemWindows(false)

        root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        term = TerminalView(this, settings)
        root.addView(term, LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f))
        keyBar = buildKeyBar()
        root.addView(keyBar, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))
        setContentView(root)

        root.setOnApplyWindowInsetsListener { v, insets ->
            val i = insets.getInsets(
                WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout() or WindowInsets.Type.ime()
            )
            v.setPadding(i.left, i.top, i.right, i.bottom)
            WindowInsets.CONSUMED
        }

        val settingsUi = SettingsUi(this, settings) { applySettings() }
        term.onOpenSettings = { settingsUi.show() }
        term.onStickyChanged = { refreshKeyColors() }
        settingsKey.setOnClickListener { settingsUi.show() }

        applySettings()
        term.requestFocus()
    }

    override fun onDestroy() {
        term.destroy()
        super.onDestroy()
    }

    /** Push preferences to every part of the UI. */
    private fun applySettings() {
        val bg = settings.bg or 0xFF000000.toInt()
        root.setBackgroundColor(bg)
        window.decorView.setBackgroundColor(bg)
        keyBar.visibility = if (settings.extraKeys) View.VISIBLE else View.GONE
        refreshKeyColors()
        term.applySettings()

        // Dark icons on light themes, light icons on dark ones.
        val light = luminance(settings.bg) > 0.5
        val mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or
            WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
        window.insetsController?.setSystemBarsAppearance(if (light) mask else 0, mask)
    }

    private fun luminance(rgb: Int): Double {
        val r = (rgb shr 16 and 0xFF) / 255.0
        val g = (rgb shr 8 and 0xFF) / 255.0
        val b = (rgb and 0xFF) / 255.0
        return 0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    private fun refreshKeyColors() {
        val fg = settings.fg or 0xFF000000.toInt()
        val accent = settings.cursor or 0xFF000000.toInt()
        for (k in keyViews) k.setTextColor(fg)
        if (term.ctrlSticky) ctrlKey.setTextColor(accent)
        if (term.altSticky) altKey.setTextColor(accent)
        ctrlKey.typeface = Typeface.create(Typeface.MONOSPACE, if (term.ctrlSticky) Typeface.BOLD else Typeface.NORMAL)
        altKey.typeface = Typeface.create(Typeface.MONOSPACE, if (term.altSticky) Typeface.BOLD else Typeface.NORMAL)
    }

    // ───────────────────────── extra keys bar ─────────────────────────

    private fun dp(v: Int): Int =
        TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

    private fun buildKeyBar(): HorizontalScrollView {
        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }

        fun key(label: String, onClick: () -> Unit): TextView {
            val ripple = TypedValue()
            theme.resolveAttribute(android.R.attr.selectableItemBackground, ripple, true)
            val tv = TextView(this).apply {
                text = label
                typeface = Typeface.MONOSPACE
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
                gravity = Gravity.CENTER
                minWidth = dp(46)
                setPadding(dp(10), dp(10), dp(10), dp(10))
                setBackgroundResource(ripple.resourceId)
                isFocusable = false // never steal keyboard focus from the terminal
                setOnClickListener { onClick() }
            }
            keyViews.add(tv)
            row.addView(tv, LinearLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT))
            return tv
        }

        fun special(label: String, keyCode: Int) = key(label) { term.sendKey(keyCode) }
        fun char(label: String) = key(label) { term.sendText(label) }

        special("ESC", KeyEvent.KEYCODE_ESCAPE)
        special("TAB", KeyEvent.KEYCODE_TAB)
        ctrlKey = key("CTRL") {
            term.ctrlSticky = !term.ctrlSticky
            refreshKeyColors()
        }
        altKey = key("ALT") {
            term.altSticky = !term.altSticky
            refreshKeyColors()
        }
        special("←", KeyEvent.KEYCODE_DPAD_LEFT)
        special("↓", KeyEvent.KEYCODE_DPAD_DOWN)
        special("↑", KeyEvent.KEYCODE_DPAD_UP)
        special("→", KeyEvent.KEYCODE_DPAD_RIGHT)
        char("-")
        char("/")
        char("|")
        char("~")
        special("HOME", KeyEvent.KEYCODE_MOVE_HOME)
        special("END", KeyEvent.KEYCODE_MOVE_END)
        special("PGUP", KeyEvent.KEYCODE_PAGE_UP)
        special("PGDN", KeyEvent.KEYCODE_PAGE_DOWN)
        settingsKey = key("⚙") {}

        return HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            addView(row)
        }
    }
}
