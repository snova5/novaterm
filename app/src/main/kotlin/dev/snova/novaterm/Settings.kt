package dev.snova.novaterm

import android.content.Context

/** A color scheme: default text/background/cursor + the 16 ANSI colors (RGB). */
class Theme(val name: String, val fg: Int, val bg: Int, val cursor: Int, val ansi: IntArray)

object Themes {
    val ALL = listOf(
        Theme(
            "Nova", 0xE4E4EF, 0x0E0E16, 0xB98CFF, intArrayOf(
                0x1B1B26, 0xF2617A, 0x7CD992, 0xF4C96B, 0x6FA3F7, 0xC48CFF, 0x5FD7D7, 0xC8C8D8,
                0x5A5A72, 0xFF8197, 0x9CF0B0, 0xFFDD8E, 0x93BCFF, 0xDAB0FF, 0x8EF0F0, 0xFFFFFF,
            )
        ),
        Theme(
            "Dracula", 0xF8F8F2, 0x282A36, 0xF8F8F2, intArrayOf(
                0x21222C, 0xFF5555, 0x50FA7B, 0xF1FA8C, 0xBD93F9, 0xFF79C6, 0x8BE9FD, 0xF8F8F2,
                0x6272A4, 0xFF6E6E, 0x69FF94, 0xFFFFA5, 0xD6ACFF, 0xFF92DF, 0xA4FFFF, 0xFFFFFF,
            )
        ),
        Theme(
            "Gruvbox Dark", 0xEBDBB2, 0x282828, 0xEBDBB2, intArrayOf(
                0x282828, 0xCC241D, 0x98971A, 0xD79921, 0x458588, 0xB16286, 0x689D6A, 0xA89984,
                0x928374, 0xFB4934, 0xB8BB26, 0xFABD2F, 0x83A598, 0xD3869B, 0x8EC07C, 0xEBDBB2,
            )
        ),
        Theme(
            "Nord", 0xD8DEE9, 0x2E3440, 0xD8DEE9, intArrayOf(
                0x3B4252, 0xBF616A, 0xA3BE8C, 0xEBCB8B, 0x81A1C1, 0xB48EAD, 0x88C0D0, 0xE5E9F0,
                0x4C566A, 0xBF616A, 0xA3BE8C, 0xEBCB8B, 0x81A1C1, 0xB48EAD, 0x8FBCBB, 0xECEFF4,
            )
        ),
        Theme(
            "Tokyo Night", 0xC0CAF5, 0x1A1B26, 0xC0CAF5, intArrayOf(
                0x15161E, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xA9B1D6,
                0x414868, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xC0CAF5,
            )
        ),
        Theme(
            "Catppuccin Mocha", 0xCDD6F4, 0x1E1E2E, 0xF5E0DC, intArrayOf(
                0x45475A, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xBAC2DE,
                0x585B70, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xA6ADC8,
            )
        ),
        Theme(
            "Solarized Dark", 0x839496, 0x002B36, 0x93A1A1, intArrayOf(
                0x073642, 0xDC322F, 0x859900, 0xB58900, 0x268BD2, 0xD33682, 0x2AA198, 0xEEE8D5,
                0x002B36, 0xCB4B16, 0x586E75, 0x657B83, 0x839496, 0x6C71C4, 0x93A1A1, 0xFDF6E3,
            )
        ),
        Theme(
            "Matrix", 0x00FF41, 0x000000, 0x00FF41, intArrayOf(
                0x000000, 0x008F11, 0x00C030, 0x00E038, 0x00A020, 0x00D035, 0x30FF70, 0x80FFA0,
                0x003B00, 0x00B018, 0x20E050, 0x40FF60, 0x10C040, 0x30F060, 0x70FF98, 0xC0FFD0,
            )
        ),
        Theme(
            "One Light", 0x383A42, 0xFAFAFA, 0x526FFF, intArrayOf(
                0x383A42, 0xE45649, 0x50A14F, 0xC18401, 0x4078F2, 0xA626A4, 0x0184BC, 0xA0A1A7,
                0x696C77, 0xE45649, 0x50A14F, 0xC18401, 0x4078F2, 0xA626A4, 0x0184BC, 0x090A0B,
            )
        ),
    )
}

/** Persistent user preferences. */
class Settings(ctx: Context) {
    private val p = ctx.getSharedPreferences("novaterm", Context.MODE_PRIVATE)

    var themeIndex: Int
        get() = p.getInt("theme", 0).coerceIn(0, Themes.ALL.size - 1)
        set(v) { p.edit().putInt("theme", v).apply() }

    val theme: Theme get() = Themes.ALL[themeIndex]

    var fontSp: Float
        get() = p.getFloat("fontSp", 13f).coerceIn(MIN_FONT, MAX_FONT)
        set(v) { p.edit().putFloat("fontSp", v.coerceIn(MIN_FONT, MAX_FONT)).apply() }

    /** 0 = block, 1 = underline, 2 = bar */
    var cursorStyle: Int
        get() = p.getInt("cursorStyle", 0).coerceIn(0, 2)
        set(v) { p.edit().putInt("cursorStyle", v).apply() }

    var cursorBlink: Boolean
        get() = p.getBoolean("cursorBlink", true)
        set(v) { p.edit().putBoolean("cursorBlink", v).apply() }

    var extraKeys: Boolean
        get() = p.getBoolean("extraKeys", true)
        set(v) { p.edit().putBoolean("extraKeys", v).apply() }

    var keepScreenOn: Boolean
        get() = p.getBoolean("keepScreenOn", false)
        set(v) { p.edit().putBoolean("keepScreenOn", v).apply() }

    var shell: String
        get() = p.getString("shell", DEFAULT_SHELL) ?: DEFAULT_SHELL
        set(v) { p.edit().putString("shell", v.trim().ifEmpty { DEFAULT_SHELL }).apply() }

    /** Per-color override of the theme (key: "fg", "bg" or "cursor"); null = use theme. */
    fun customColor(key: String): Int? = if (p.contains("c_$key")) p.getInt("c_$key", 0) else null

    fun setCustomColor(key: String, rgb: Int?) {
        val e = p.edit()
        if (rgb == null) e.remove("c_$key") else e.putInt("c_$key", rgb and 0xFFFFFF)
        e.apply()
    }

    fun clearCustomColors() {
        p.edit().remove("c_fg").remove("c_bg").remove("c_cursor").apply()
    }

    val fg: Int get() = customColor("fg") ?: theme.fg
    val bg: Int get() = customColor("bg") ?: theme.bg
    val cursor: Int get() = customColor("cursor") ?: theme.cursor

    /** Layout expected by Native.setTheme. */
    fun themeInts(): IntArray = intArrayOf(fg, bg, cursor) + theme.ansi

    fun resetAll() {
        p.edit().clear().apply()
    }

    companion object {
        const val DEFAULT_SHELL = "/system/bin/sh"
        const val MIN_FONT = 6f
        const val MAX_FONT = 40f
    }
}
