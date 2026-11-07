package com.wonderhoylabs.airi.viewer

import android.app.Activity
import android.app.ActivityManager
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.view.Gravity
import android.view.PointerIcon
import android.view.SurfaceHolder
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.WindowInsets
import android.view.WindowInsetsAnimation
import android.widget.Button
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import android.window.OnBackInvokedDispatcher
import com.wonderhoylabs.airi.R
import com.wonderhoylabs.airi.agent.AgentConnection
import com.wonderhoylabs.airi.agent.AgentSetup
import com.wonderhoylabs.airi.agent.Message
import com.wonderhoylabs.airi.agent.Payload
import com.wonderhoylabs.airi.agent.Protocol
import com.wonderhoylabs.airi.agent.WrongTokenException
import com.wonderhoylabs.airi.launcher.AppIcons
import java.io.IOException
import kotlin.concurrent.thread
import kotlin.math.roundToInt

/** One Linux window, in its own Android task. Each app's window gets its own copy of this. */
class ViewerActivity : Activity(), SurfaceHolder.Callback, LinuxView.Input {
    private lateinit var area: FrameLayout
    private lateinit var linuxView: LinuxView
    private lateinit var overlay: LinearLayout
    private lateinit var spinner: ProgressBar
    private lateinit var status: TextView
    private lateinit var retry: Button
    private lateinit var keyboard: ImageButton

    private var desktopId = ""
    private var windowId = 0
    private val native = NativeView.create()

    @Volatile private var connection: AgentConnection? = null
    @Volatile private var opened = false
    @Volatile private var closing = false
    @Volatile private var areaWidth = 0
    @Volatile private var areaHeight = 0
    @Volatile private var keyboardCover = 0
    @Volatile private var offeredWidth = 0
    @Volatile private var offeredHeight = 0
    @Volatile private var frameWidth = 0
    @Volatile private var frameHeight = 0
    @Volatile private var visible = false
    @Volatile private var focused = false
    private val sizeLock = Object()
    private var session: Thread? = null
    @Volatile private var keyboardWanted = false
    @Volatile private var overlayShown = true
    private var lastClipboard: String? = null
    private var appIcon: Bitmap? = null

    private val scale: Float get() = resources.displayMetrics.density

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Tells the app on the launcher that this closed on purpose, if it's what opened this.
        setResult(RESULT_OK)
        val uri = intent.data
        desktopId = uri?.lastPathSegment.orEmpty()
        windowId = uri?.getQueryParameter("window")?.toIntOrNull() ?: 0

        buildViews()
        appIcon = AppIcons.load(this, desktopId)?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
        setLabel(AppIcons.name(this, desktopId) ?: getString(R.string.app_name))

        // Back leaves the app running, the same as it does for an Android app you came back to.
        onBackInvokedDispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT) {
            moveTaskToBack(true)
        }
        startSession()
    }

    private fun buildViews() {
        val root = FrameLayout(this)
        root.setBackgroundColor(Color.BLACK)
        // The surface fills this area and keeps its size when the keyboard opens, which just slides
        // over it. Resizing a surface and its contents can't happen in step, and it shows.
        area = FrameLayout(this)
        area.addOnLayoutChangeListener { _, left, top, right, bottom, _, _, _, _ ->
            onAreaSize(right - left, bottom - top)
        }
        root.addView(area, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        linuxView = LinuxView(this, this)
        linuxView.holder.addCallback(this)
        area.addView(linuxView, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT, Gravity.TOP or Gravity.CENTER_HORIZONTAL))

        val padding = (24 * scale).toInt()
        overlay = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(padding, padding, padding, padding)
            setBackgroundColor(Color.BLACK)
        }
        spinner = ProgressBar(this)
        status = TextView(this).apply {
            setTextColor(Color.WHITE)
            textSize = 16f
            gravity = Gravity.CENTER
            setPadding(0, padding, 0, padding)
        }
        retry = Button(this).apply {
            text = getString(R.string.viewer_try_again)
            visibility = View.GONE
            setOnClickListener { startSession() }
        }
        overlay.addView(spinner)
        overlay.addView(status)
        overlay.addView(retry)
        root.addView(overlay, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))

        val size = (44 * scale).toInt()
        keyboard = ImageButton(this).apply {
            setImageResource(R.drawable.ic_keyboard)
            contentDescription = getString(R.string.viewer_keyboard)
            setBackgroundResource(R.drawable.keyboard_button)
            alpha = 0.55f
            visibility = View.GONE
            setOnClickListener { showKeyboard(!keyboardWanted) }
        }
        root.addView(
            keyboard,
            FrameLayout.LayoutParams(size, size, Gravity.END or Gravity.CENTER_VERTICAL).apply {
                marginEnd = (6 * scale).toInt()
            },
        )

        root.setOnApplyWindowInsetsListener { view, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            keyboardWanted = insets.isVisible(WindowInsets.Type.ime())
            keyboardCover = maxOf(0, insets.getInsets(WindowInsets.Type.ime()).bottom - bars.bottom)
            offerSize()
            WindowInsets.CONSUMED
        }
        // Having one of these makes Android report where the keyboard will end up as it starts moving,
        // not after it's gone, so the app redraws during the slide instead of after it.
        root.setWindowInsetsAnimationCallback(object : WindowInsetsAnimation.Callback(DISPATCH_MODE_STOP) {
            override fun onProgress(insets: WindowInsets, running: List<WindowInsetsAnimation>) = insets
        })
        setContentView(root)
        window.setDecorFitsSystemWindows(false)
        linuxView.requestFocus()
    }

    private fun showKeyboard(show: Boolean) {
        val controller = linuxView.windowInsetsController ?: return
        linuxView.requestFocus()
        if (show) controller.show(WindowInsets.Type.ime()) else controller.hide(WindowInsets.Type.ime())
    }

    private fun updateKeyboardButton() {
        val hasKeyboard = resources.configuration.keyboard != Configuration.KEYBOARD_NOKEYS &&
            resources.configuration.hardKeyboardHidden == Configuration.HARDKEYBOARDHIDDEN_NO
        keyboard.visibility = if (opened && !hasKeyboard) View.VISIBLE else View.GONE
    }

    private fun setLabel(label: String) {
        title = label
        @Suppress("DEPRECATION")
        setTaskDescription(ActivityManager.TaskDescription(label, appIcon))
    }

    private fun showStatus(text: String, working: Boolean = true) {
        overlayShown = true
        runOnUiThread {
            overlay.visibility = View.VISIBLE
            spinner.visibility = if (working) View.VISIBLE else View.GONE
            retry.visibility = if (working) View.GONE else View.VISIBLE
            status.text = text
        }
    }

    private fun startSession() {
        if (session?.isAlive == true) return
        session = thread(name = "airi-session") {
            try {
                runSession()
            } catch (_: InterruptedException) {
                // Closing.
            }
        }
    }

    private fun connectToLinux(): AgentConnection? {
        val appName = AppIcons.name(this, desktopId) ?: getString(R.string.app_name)
        showStatus(getString(R.string.viewer_opening, appName))
        var startedLinux = false
        val deadline = System.currentTimeMillis() + LINUX_START_TIMEOUT_MS
        while (!closing) {
            try {
                return AgentSetup.connect(this)
            } catch (_: WrongTokenException) {
                showStatus(getString(R.string.viewer_wrong_token), working = false)
                return null
            } catch (_: IOException) {
                if (System.currentTimeMillis() > deadline) {
                    showStatus(getString(R.string.viewer_no_linux), working = false)
                    return null
                }
                if (!startedLinux) {
                    startedLinux = true
                    showStatus(getString(R.string.viewer_starting_linux))
                    runOnUiThread(::startLinux)
                }
                Thread.sleep(RETRY_DELAY_MS)
            }
        }
        return null
    }

    /**
     * Only the Terminal app can start Linux, and it has to be opened to do it. It keeps running in the
     * background from then on, so this comes straight back once it's had a moment to start.
     */
    private fun startLinux() {
        val terminal = AgentSetup.terminalIntent(this) ?: return
        startActivity(terminal)
        linuxView.postDelayed({
            startActivity(
                Intent(intent).addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT),
            )
        }, TERMINAL_HEAD_START_MS)
    }

    private fun runSession() {
        val agent = connectToLinux() ?: return
        connection = agent
        if (agent.protocol != Protocol.VERSION) {
            showStatus(getString(R.string.viewer_old_agent), working = false)
            agent.close()
            connection = null
            return
        }
        waitForArea()
        agent.send(
            Protocol.OPEN,
            Payload().string(desktopId).u32(windowId).u32(offeredWidth).u32(offeredHeight).f32(scale),
        )
        opened = true
        agent.send(Protocol.VISIBLE, Payload().u8(if (visible) 1 else 0))
        agent.send(Protocol.FOCUS, Payload().u8(if (focused) 1 else 0))
        try {
            while (true) handle(agent, agent.read())
        } catch (_: IOException) {
            if (!closing) showStatus(getString(R.string.viewer_lost_linux), working = false)
        } finally {
            opened = false
            connection = null
            agent.close()
        }
    }

    private fun waitForArea() {
        synchronized(sizeLock) {
            while (offeredWidth == 0) sizeLock.wait()
        }
    }

    private fun onAreaSize(width: Int, height: Int) {
        // A picture being scaled makes the surface smaller than the area, and that isn't a new area.
        if (width == 0 || height == 0 || (width == areaWidth && height == areaHeight)) return
        areaWidth = width
        areaHeight = height
        NativeView.setArea(native, width, height)
        fitPicture()
        offerSize()
    }

    /** Tells Linux how much room there is, which is whatever the keyboard isn't covering. */
    private fun offerSize() {
        val width = areaWidth
        val height = areaHeight - keyboardCover
        if (width == 0 || height <= 0 || (width == offeredWidth && height == offeredHeight)) return
        synchronized(sizeLock) {
            offeredWidth = width
            offeredHeight = height
            sizeLock.notifyAll()
        }
        sendWhileOpen(Protocol.RESIZE, Payload().u32(width).u32(height).f32(scale))
    }

    /** Scales the picture down if an app won't fit the area. Almost all of the time, it does. */
    private fun fitPicture() {
        if (frameWidth == 0 || areaWidth == 0) return
        val fit = minOf(1f, areaWidth.toFloat() / frameWidth, areaHeight.toFloat() / frameHeight)
        val width = if (fit < 1f) (frameWidth * fit).roundToInt() else MATCH_PARENT
        val height = if (fit < 1f) (frameHeight * fit).roundToInt() else MATCH_PARENT
        linuxView.pictureScale = 1 / fit
        val params = linuxView.layoutParams
        if (params.width != width || params.height != height) {
            params.width = width
            params.height = height
            linuxView.layoutParams = params
        }
    }

    private fun handle(agent: AgentConnection, message: Message) {
        when (message.type) {
            Protocol.FRAME -> {
                val size = NativeView.frame(native, message.body, message.body.limit())
                if (size < 0) throw IOException("Bad frame")
                val width = (size ushr 32).toInt()
                val height = size.toInt()
                agent.send(Protocol.FRAME_ACK)
                if (width != frameWidth || height != frameHeight) {
                    frameWidth = width
                    frameHeight = height
                    runOnUiThread(::fitPicture)
                }
                if (overlayShown) {
                    overlayShown = false
                    runOnUiThread {
                        overlay.visibility = View.GONE
                        updateKeyboardButton()
                    }
                }
            }
            Protocol.ATTACHED -> {
                message.body.getInt()
                message.string()
                val title = message.string()
                if (title.isNotEmpty()) runOnUiThread { setLabel(title) }
            }
            Protocol.TITLE -> {
                val title = message.string()
                runOnUiThread { setLabel(title) }
            }
            Protocol.IME -> {
                val show = message.body.get().toInt() != 0
                runOnUiThread { showKeyboard(show) }
            }
            Protocol.CLIPBOARD_CHANGED -> {
                val text = message.string()
                runOnUiThread {
                    lastClipboard = text
                    getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Linux", text))
                }
            }
            Protocol.NEW_WINDOW -> {
                val id = message.body.getInt()
                val desktop = message.string()
                runOnUiThread { openWindow(id, desktop) }
            }
            Protocol.CURSOR -> {
                val icon = readCursor(message)
                runOnUiThread { linuxView.pointerIcon = icon }
            }
            Protocol.CLOSED -> runOnUiThread { finishAndRemoveTask() }
            Protocol.ERROR -> showStatus(message.string(), working = false)
        }
    }

    private fun openWindow(id: Int, desktop: String) {
        val uri = Uri.Builder().scheme("airi").authority("app").appendPath(desktop.ifEmpty { "window" })
            .appendQueryParameter("window", id.toString()).build()
        startActivity(
            Intent(Intent.ACTION_VIEW, uri, this, ViewerActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT or Intent.FLAG_ACTIVITY_MULTIPLE_TASK),
        )
    }

    private fun readCursor(message: Message): PointerIcon? {
        val body = message.body
        return when (body.get().toInt()) {
            CURSOR_HIDDEN -> PointerIcon.getSystemIcon(this, PointerIcon.TYPE_NULL)
            CURSOR_NAMED -> PointerIcon.getSystemIcon(this, namedCursor(message.string()))
            else -> {
                val hotX = body.getInt().toFloat()
                val hotY = body.getInt().toFloat()
                val width = body.getInt()
                val height = body.getInt()
                if (width <= 0 || height <= 0) return null
                val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                bitmap.copyPixelsFromBuffer(body)
                PointerIcon.create(bitmap, hotX, hotY)
            }
        }
    }

    private fun namedCursor(name: String): Int = when (name) {
        "text", "vertical-text" -> PointerIcon.TYPE_TEXT
        "pointer", "grab" -> PointerIcon.TYPE_HAND
        "grabbing" -> PointerIcon.TYPE_GRABBING
        "wait", "progress" -> PointerIcon.TYPE_WAIT
        "crosshair" -> PointerIcon.TYPE_CROSSHAIR
        "help" -> PointerIcon.TYPE_HELP
        "not-allowed", "no-drop" -> PointerIcon.TYPE_NO_DROP
        "ew-resize", "e-resize", "w-resize", "col-resize" -> PointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW
        "ns-resize", "n-resize", "s-resize", "row-resize" -> PointerIcon.TYPE_VERTICAL_DOUBLE_ARROW
        "nesw-resize", "ne-resize", "sw-resize" -> PointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW
        "nwse-resize", "nw-resize", "se-resize" -> PointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW
        else -> PointerIcon.TYPE_ARROW
    }

    private fun sendWhileOpen(type: Int, payload: Payload? = null) {
        if (opened) connection?.send(type, payload)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        NativeView.setSurface(native, holder.surface)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        // Attaching again redraws the picture, so that it doesn't blink while the surface changes.
        NativeView.setSurface(native, holder.surface)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        NativeView.setSurface(native, null)
    }

    override fun onStart() {
        super.onStart()
        visible = true
        sendWhileOpen(Protocol.VISIBLE, Payload().u8(1))
    }

    override fun onStop() {
        super.onStop()
        visible = false
        sendWhileOpen(Protocol.VISIBLE, Payload().u8(0))
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        focused = hasFocus
        sendWhileOpen(Protocol.FOCUS, Payload().u8(if (hasFocus) 1 else 0))
        // Android only lets the app with focus read the clipboard, so this is the moment to look.
        if (hasFocus) shareClipboard()
    }

    private fun shareClipboard() {
        val clip = getSystemService(ClipboardManager::class.java).primaryClip ?: return
        if (clip.itemCount == 0) return
        val text = clip.getItemAt(0).coerceToText(this)?.toString().orEmpty()
        if (text.isEmpty() || text == lastClipboard) return
        lastClipboard = text
        sendWhileOpen(Protocol.CLIPBOARD, Payload().string(text))
    }

    override fun onDestroy() {
        closing = true
        linuxView.holder.removeCallback(this)
        val agent = connection
        if (isFinishing && agent != null) {
            // Closing the Android window closes the Linux one, the way an app's own close button would.
            agent.send(Protocol.CLOSE)
            agent.finish()
        } else {
            agent?.close()
        }
        session?.interrupt()
        val handle = native
        // The session can be in the middle of a frame, so the native side has to outlive it.
        thread(name = "airi-cleanup") {
            session?.join(CLEANUP_WAIT_MS)
            NativeView.destroy(handle)
        }
        super.onDestroy()
    }

    override fun pointer(kind: Int, x: Float, y: Float, button: Int, scrollX: Float, scrollY: Float) {
        sendWhileOpen(Protocol.POINTER, Payload().u8(kind).f32(x).f32(y).u32(button).f32(scrollX).f32(scrollY))
    }

    override fun touch(kind: Int, id: Int, x: Float, y: Float) {
        sendWhileOpen(Protocol.TOUCH, Payload().u8(kind).u32(id).f32(x).f32(y))
    }

    override fun key(code: Int, pressed: Boolean) {
        sendWhileOpen(Protocol.KEY, Payload().u32(code).u8(if (pressed) 1 else 0))
    }

    override fun text(text: String) {
        sendWhileOpen(Protocol.TEXT, Payload().string(text))
    }

    private companion object {
        const val LINUX_START_TIMEOUT_MS = 120_000L
        const val RETRY_DELAY_MS = 500L
        const val TERMINAL_HEAD_START_MS = 1500L
        const val CLEANUP_WAIT_MS = 5000L
        const val CURSOR_HIDDEN = 0
        const val CURSOR_NAMED = 1
    }
}
