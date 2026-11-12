package dev.snova.novaterm

/** JNI bridge to the Rust core (rust/src/lib.rs). */
object Native {
    init {
        System.loadLibrary("novaterm")
    }

    /** Starts a process on a new PTY. Returns a handle, or 0 on failure. */
    @JvmStatic external fun create(
        program: String, argv: Array<String>, env: Array<String>, cwd: String, rows: Int, cols: Int,
    ): Long

    /** Sends bytes to the process (keyboard input). */
    @JvmStatic external fun write(h: Long, data: ByteArray)

    /** Feeds bytes directly to the emulator, bypassing the process. */
    @JvmStatic external fun feedLocal(h: Long, data: ByteArray)

    @JvmStatic external fun resize(h: Long, rows: Int, cols: Int)

    /** Blocks until the screen changes (-1) or the process exits (exit code >= 0). */
    @JvmStatic external fun waitUpdate(h: Long): Int

    /**
     * [rows, cols, cursorRow (-1 hidden), cursorCol, scrollbackLen, flags, defaultBg, cursorColor]
     * followed by rows*cols cells of [codepoint, fgARGB, bgARGB, attr].
     */
    @JvmStatic external fun snapshot(h: Long, scroll: Int): IntArray

    /** colors = [fg, bg, cursor, ansi0..ansi15] as RGB. */
    @JvmStatic external fun setTheme(h: Long, colors: IntArray)

    @JvmStatic external fun hangup(h: Long)

    /** Only after the process exited and waitUpdate returned its exit code. */
    @JvmStatic external fun free(h: Long)
}
