package com.wonderhoylabs.airi.agent

import java.io.ByteArrayOutputStream
import java.io.Closeable
import java.io.DataInputStream
import java.io.EOFException
import java.io.IOException
import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.channels.SocketChannel
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.concurrent.LinkedBlockingQueue
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec
import kotlin.concurrent.thread

/** The agent answered, but doesn't have our token. It was probably set up by another install. */
class WrongTokenException : IOException("The agent has a different token")

/** Thrown when something other than the agent answers on its port. */
class NotAgentException : IOException("That isn't the Airi agent")

/** Builds a message's payload, in the agent's byte order. */
class Payload {
	private val out = ByteArrayOutputStream()

	fun u8(value: Int) = apply { out.write(value) }

	fun u32(value: Int) = apply {
		repeat(4) { out.write(value ushr (it * 8)) }
	}

	fun f32(value: Float) = u32(value.toRawBits())

	fun string(value: String) = apply {
		val bytes = value.toByteArray()
		u32(bytes.size)
		out.write(bytes)
	}

	fun bytes(value: ByteArray) = apply { out.write(value) }

	fun toByteArray(): ByteArray = out.toByteArray()
}

/** A message from the agent. Its buffer is only good until the next read. */
class Message(val type: Int, val body: ByteBuffer) {
	fun string(): String {
		val bytes = ByteArray(body.getInt())
		body.get(bytes)
		return String(bytes)
	}

	fun rest(): ByteArray = ByteArray(body.remaining()).also(body::get)
}

/**
 * One connection to the agent. Reads block, so they belong on their own thread, but sending only
 * joins a queue and is safe anywhere.
 */
class AgentConnection private constructor(
	private val channel: SocketChannel,
	val agentVersion: String,
	val protocol: Int,
) : Closeable {
	private val header = ByteBuffer.allocateDirect(5).order(ByteOrder.LITTLE_ENDIAN)
	private var body = ByteBuffer.allocateDirect(1 shl 20).order(ByteOrder.LITTLE_ENDIAN)
	private val outgoing = LinkedBlockingQueue<ByteBuffer>()

	private val writer = thread(name = "airi-send", isDaemon = true) {
		try {
			while (true) {
				val message = outgoing.take()
				if (message === END) break
				while (message.hasRemaining()) channel.write(message)
			}
		} catch (_: IOException) {
			// The reading side notices as well, and that's where it gets handled.
		} catch (_: InterruptedException) {
			// Closing.
		}
		runCatching { channel.close() }
	}

	fun read(): Message = Companion.read(channel, header, ::bodyFor)

	private fun bodyFor(size: Int): ByteBuffer {
		if (body.capacity() < size) {
			body = ByteBuffer.allocateDirect(size + size / 4).order(ByteOrder.LITTLE_ENDIAN)
		}
		body.clear().limit(size)
		return body
	}

	fun send(type: Int, payload: Payload? = null) {
		val bytes = payload?.toByteArray() ?: ByteArray(0)
		val message = ByteBuffer.allocate(5 + bytes.size).order(ByteOrder.LITTLE_ENDIAN)
		message.putInt(bytes.size).put(type.toByte()).put(bytes).flip()
		outgoing.put(message)
	}

	/** Closes once everything queued has gone out. */
	fun finish() {
		outgoing.put(END)
	}

	override fun close() {
		writer.interrupt()
		runCatching { channel.close() }
	}

	companion object {
		private val END = ByteBuffer.allocate(0)
		private const val TIMEOUT_MS = 3000
		private const val MAX_MESSAGE = 128 * 1024 * 1024

		private fun read(channel: SocketChannel, header: ByteBuffer, body: (Int) -> ByteBuffer): Message {
			header.clear()
			readFully(channel, header)
			header.flip()
			val size = header.getInt()
			val type = header.get().toInt() and 0xff
			if (size < 0 || size > MAX_MESSAGE) throw IOException("A message claimed to be $size bytes")
			val buffer = body(size)
			readFully(channel, buffer)
			buffer.flip()
			return Message(type, buffer)
		}

		private fun readFully(channel: SocketChannel, buffer: ByteBuffer) {
			while (buffer.hasRemaining()) {
				if (channel.read(buffer) < 0) throw EOFException()
			}
		}

		private fun proof(token: ByteArray, label: String, serverNonce: ByteArray, clientNonce: ByteArray): ByteArray {
			val mac = Mac.getInstance("HmacSHA256")
			mac.init(SecretKeySpec(token, "HmacSHA256"))
			mac.update(label.toByteArray())
			mac.update(serverNonce)
			return mac.doFinal(clientNonce)
		}

		/** Connects and proves to each other that both ends hold the token, without sending it. */
		fun connect(address: InetSocketAddress, token: ByteArray): AgentConnection {
			val channel = SocketChannel.open()
			try {
				val socket = channel.socket()
				socket.tcpNoDelay = true
				socket.connect(address, TIMEOUT_MS)
				// The Terminal app accepts on a forwarded port even with nothing behind it, so a read
				// here can fail or hang, and only a timeout tells those apart from a slow agent.
				socket.soTimeout = TIMEOUT_MS
				val input = DataInputStream(socket.getInputStream())
				val hello = ByteArray(38)
				input.readFully(hello)
				if (String(hello, 0, 4) != "AIRI") throw NotAgentException()
				val serverNonce = hello.copyOfRange(6, 38)
				val clientNonce = ByteArray(32).also(SecureRandom()::nextBytes)

				val reply = clientNonce + proof(token, "airi client", serverNonce, clientNonce)
				channel.write(ByteBuffer.wrap(reply))
				val answer = ByteArray(32)
				try {
					input.readFully(answer)
				} catch (_: EOFException) {
					throw WrongTokenException()
				}
				val expected = proof(token, "airi server", serverNonce, clientNonce)
				if (!MessageDigest.isEqual(answer, expected)) throw WrongTokenException()
				socket.soTimeout = 0

				val infoHeader = ByteBuffer.allocateDirect(5).order(ByteOrder.LITTLE_ENDIAN)
				val infoBody = ByteBuffer.allocateDirect(4096).order(ByteOrder.LITTLE_ENDIAN)
				val info = read(channel, infoHeader) { size ->
					if (size > infoBody.capacity()) throw NotAgentException()
					infoBody.clear().limit(size)
					infoBody
				}
				if (info.type != Protocol.INFO) throw NotAgentException()
				val version = info.string()
				return AgentConnection(channel, version, info.body.getInt())
			} catch (e: Throwable) {
				runCatching { channel.close() }
				throw e
			}
		}
	}
}
