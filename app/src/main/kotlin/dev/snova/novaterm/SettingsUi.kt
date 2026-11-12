package dev.snova.novaterm

import android.app.Activity
import android.app.AlertDialog
import android.text.InputType
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.SeekBar
import android.widget.TextView
import android.widget.Toast

/** Settings screen built from plain dialogs; every change is applied live through [onChange]. */
class SettingsUi(
    private val activity: Activity,
    private val settings: Settings,
    private val onChange: () -> Unit,
) {
    private val cursorNames = arrayOf("Block", "Underline", "Vertical bar")

    private fun yesNo(b: Boolean) = if (b) "on" else "off"
    private fun hex(rgb: Int) = "#%06X".format(rgb and 0xFFFFFF)
    private fun colorLabel(key: String, value: Int) =
        hex(value) + if (settings.customColor(key) != null) " (custom)" else " (from theme)"

    fun show() {
        val items = arrayOf(
            "Theme: ${settings.theme.name}",
            "Text color: ${colorLabel("fg", settings.fg)}",
            "Background color: ${colorLabel("bg", settings.bg)}",
            "Cursor color: ${colorLabel("cursor", settings.cursor)}",
            "Font size: ${settings.fontSp.toInt()}",
            "Cursor style: ${cursorNames[settings.cursorStyle]}",
            "Blinking cursor: ${yesNo(settings.cursorBlink)}",
            "Extra keys bar: ${yesNo(settings.extraKeys)}",
            "Keep screen on: ${yesNo(settings.keepScreenOn)}",
            "Shell: ${settings.shell}",
            "Reset all settings",
        )
        AlertDialog.Builder(activity)
            .setTitle("Settings")
            .setItems(items) { _, which ->
                when (which) {
                    0 -> pickTheme()
                    1 -> pickColor("fg", "Text color")
                    2 -> pickColor("bg", "Background color")
                    3 -> pickColor("cursor", "Cursor color")
                    4 -> pickFont()
                    5 -> pickCursor()
                    6 -> toggle { settings.cursorBlink = !settings.cursorBlink }
                    7 -> toggle { settings.extraKeys = !settings.extraKeys }
                    8 -> toggle { settings.keepScreenOn = !settings.keepScreenOn }
                    9 -> pickShell()
                    10 -> confirmReset()
                }
            }
            .setPositiveButton("Close", null)
            .show()
    }

    private fun toggle(change: () -> Unit) {
        change()
        onChange()
        show()
    }

    private fun pickTheme() {
        val names = Themes.ALL.map { it.name }.toTypedArray()
        AlertDialog.Builder(activity)
            .setTitle("Theme")
            .setSingleChoiceItems(names, settings.themeIndex) { dialog, which ->
                settings.themeIndex = which
                settings.clearCustomColors() // a new theme replaces custom colors
                onChange()
                dialog.dismiss()
                show()
            }
            .setNegativeButton("Back") { _, _ -> show() }
            .show()
    }

    private fun pickCursor() {
        AlertDialog.Builder(activity)
            .setTitle("Cursor style")
            .setSingleChoiceItems(cursorNames, settings.cursorStyle) { dialog, which ->
                settings.cursorStyle = which
                onChange()
                dialog.dismiss()
                show()
            }
            .setNegativeButton("Back") { _, _ -> show() }
            .show()
    }

    private fun padded(view: android.view.View): FrameLayout {
        val pad = (20 * activity.resources.displayMetrics.density).toInt()
        return FrameLayout(activity).apply {
            setPadding(pad, pad / 2, pad, 0)
            addView(view)
        }
    }

    private fun pickColor(key: String, title: String) {
        val current = when (key) {
            "fg" -> settings.fg
            "bg" -> settings.bg
            else -> settings.cursor
        }
        val input = EditText(activity).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            hint = "#RRGGBB"
            setText(hex(current))
            setSelection(text.length)
        }
        AlertDialog.Builder(activity)
            .setTitle(title)
            .setView(padded(input))
            .setPositiveButton("OK") { _, _ ->
                val rgb = parseHex(input.text.toString())
                if (rgb == null) {
                    Toast.makeText(activity, "Invalid color — use the form #RRGGBB", Toast.LENGTH_LONG).show()
                } else {
                    settings.setCustomColor(key, rgb)
                    onChange()
                }
                show()
            }
            .setNeutralButton("From theme") { _, _ ->
                settings.setCustomColor(key, null)
                onChange()
                show()
            }
            .setNegativeButton("Back") { _, _ -> show() }
            .show()
    }

    private fun parseHex(raw: String): Int? {
        var s = raw.trim().removePrefix("#")
        if (s.length == 3) s = s.map { "$it$it" }.joinToString("") // #RGB shorthand
        if (s.length != 6) return null
        return s.toIntOrNull(16)
    }

    private fun pickFont() {
        val lo = 8
        val hi = 32
        val label = TextView(activity).apply { text = "${settings.fontSp.toInt()}" }
        val bar = SeekBar(activity).apply {
            max = hi - lo
            progress = settings.fontSp.toInt().coerceIn(lo, hi) - lo
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(sb: SeekBar, value: Int, fromUser: Boolean) {
                    if (!fromUser) return
                    settings.fontSp = (value + lo).toFloat()
                    label.text = "${value + lo}"
                    onChange() // live preview behind the dialog
                }

                override fun onStartTrackingTouch(sb: SeekBar) {}
                override fun onStopTrackingTouch(sb: SeekBar) {}
            })
        }
        val box = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            addView(label)
            addView(bar)
        }
        AlertDialog.Builder(activity)
            .setTitle("Font size")
            .setMessage("You can also pinch the screen with two fingers.")
            .setView(padded(box))
            .setPositiveButton("OK") { _, _ -> show() }
            .show()
    }

    private fun pickShell() {
        val input = EditText(activity).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            hint = Settings.DEFAULT_SHELL
            setText(settings.shell)
            setSelection(text.length)
        }
        AlertDialog.Builder(activity)
            .setTitle("Shell")
            .setMessage("Full path of the program started in the terminal. Applies to the next session (type exit, then press Enter).")
            .setView(padded(input))
            .setPositiveButton("OK") { _, _ ->
                settings.shell = input.text.toString()
                show()
            }
            .setNegativeButton("Back") { _, _ -> show() }
            .show()
    }

    private fun confirmReset() {
        AlertDialog.Builder(activity)
            .setTitle("Reset all settings?")
            .setPositiveButton("Reset") { _, _ ->
                settings.resetAll()
                onChange()
                show()
            }
            .setNegativeButton("Back") { _, _ -> show() }
            .show()
    }
}
