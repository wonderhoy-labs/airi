package com.wonderhoylabs.airi.agent

import android.os.ParcelFileDescriptor
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import java.io.Closeable
import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.nio.channels.SocketChannel

/**
 * What a connection to the agent runs over. Closing it from another thread wakes a read that's waiting, which is how
 * a connection that hangs is cut off.
 */
interface Link : Closeable {
	/** Like a channel's read: how many bytes came, or -1 at the end. */
	fun read(buffer: ByteBuffer): Int

	fun write(buffer: ByteBuffer): Int
}

/** TCP to the port Android's terminal forwards. */
class TcpLink private constructor(private val channel: SocketChannel) : Link {
	override fun read(buffer: ByteBuffer) = channel.read(buffer)

	override fun write(buffer: ByteBuffer) = channel.write(buffer)

	override fun close() = channel.close()

	companion object {
		fun connect(address: InetSocketAddress, timeoutMs: Int): TcpLink {
			val channel = SocketChannel.open()
			try {
				channel.socket().tcpNoDelay = true
				channel.socket().connect(address, timeoutMs)
				return TcpLink(channel)
			} catch (error: Throwable) {
				runCatching { channel.close() }
				throw error
			}
		}
	}
}

/**
 * A socket that another app handed over, such as Minori's connection to a machine. It reads and writes the descriptor
 * directly, because a stream or channel on it would close it as well, behind the back of the [ParcelFileDescriptor]
 * that owns it.
 */
class SocketLink(private val socket: ParcelFileDescriptor) : Link {
	override fun read(buffer: ByteBuffer): Int = try {
		val count = Os.read(socket.fileDescriptor, buffer)
		if (count == 0 && buffer.hasRemaining()) -1 else count
	} catch (error: ErrnoException) {
		throw error.rethrowAsIOException()
	}

	override fun write(buffer: ByteBuffer): Int = try {
		Os.write(socket.fileDescriptor, buffer)
	} catch (error: ErrnoException) {
		throw error.rethrowAsIOException()
	}

	override fun close() {
		// Shutting the socket down is what wakes a read that's waiting on it; closing alone might not.
		try {
			Os.shutdown(socket.fileDescriptor, OsConstants.SHUT_RDWR)
		} catch (_: ErrnoException) {
			// Already closed.
		}
		runCatching { socket.close() }
	}
}
