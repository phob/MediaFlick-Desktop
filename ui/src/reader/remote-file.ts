import type { BookFile } from "foliate-js/view.js"
import { ApiError, responseError } from "@/lib/api/client"
import { link } from "@/lib/api/routes"

const BLOCK = 1024 * 1024
/** The native route answers at most 8 MiB, so one request covers at most this many blocks. */
const BLOCKS_PER_REQUEST = 8
const CACHED_BLOCKS = 48

/**
 * A book's file on the server, shaped like the `Blob` foliate, pdf.js and
 * zip.js read from. Reads go to the native `/api/item/{id}/book/file` route
 * in 1 MiB blocks, so opening a 24 MB EPUB fetches its central directory and
 * the chapters shown, not the whole file. Adjacent missing blocks share one
 * request, a block being fetched is shared by every read that needs it, and
 * the most recently used 48 blocks stay cached.
 */
export class RemoteFile implements BookFile {
  readonly type = ""
  readonly name: string
  readonly size: number
  readonly #itemId: string
  readonly #signal: AbortSignal
  #blocks = new Map<number, Promise<Uint8Array>>()

  private constructor(itemId: string, name: string, size: number, signal: AbortSignal) {
    this.#itemId = itemId
    this.name = name
    this.size = size
    this.#signal = signal
  }

  /** Opens the file, learning its size from the first block's `Content-Range`. */
  static async open(itemId: string, name: string, signal: AbortSignal): Promise<RemoteFile> {
    const first = await fetchRange(itemId, 0, BLOCK - 1, signal)
    const file = new RemoteFile(itemId, name, first.total, signal)
    file.#remember(0, Promise.resolve(first.bytes))
    return file
  }

  slice(start = 0, end = this.size): { arrayBuffer(): Promise<ArrayBuffer> } {
    const from = Math.max(0, Math.min(start, this.size))
    const to = Math.max(from, Math.min(end, this.size))
    return { arrayBuffer: () => this.#read(from, to) }
  }

  arrayBuffer(): Promise<ArrayBuffer> {
    return this.#read(0, this.size)
  }

  /** The whole file, fetched in sequence, for decoders that need all of it at once. */
  async download(onProgress: (fraction: number) => void): Promise<File> {
    const parts: Uint8Array<ArrayBuffer>[] = []
    const step = BLOCK * BLOCKS_PER_REQUEST
    for (let start = 0; start < this.size; start += step) {
      const end = Math.min(start + step, this.size) - 1
      parts.push((await fetchRange(this.#itemId, start, end, this.#signal)).bytes)
      onProgress((end + 1) / this.size)
    }
    return new File(parts, this.name)
  }

  async #read(start: number, end: number): Promise<ArrayBuffer> {
    const out = new Uint8Array(end - start)
    if (end === start) return out.buffer
    const first = Math.floor(start / BLOCK)
    const last = Math.floor((end - 1) / BLOCK)
    this.#fetchMissing(first, last)
    for (let index = first; index <= last; index += 1) {
      const block = await this.#block(index)
      const blockStart = index * BLOCK
      const from = Math.max(start, blockStart) - blockStart
      const to = Math.min(end, blockStart + block.length) - blockStart
      out.set(block.subarray(from, to), blockStart + from - start)
    }
    return out.buffer
  }

  #fetchMissing(first: number, last: number) {
    let run: number[] = []
    const flush = () => {
      if (run.length === 0) return
      const blocks = run
      run = []
      const start = blocks[0] * BLOCK
      const end = Math.min((blocks[blocks.length - 1] + 1) * BLOCK, this.size) - 1
      const response = fetchRange(this.#itemId, start, end, this.#signal)
      for (const index of blocks) {
        const offset = (index - blocks[0]) * BLOCK
        this.#remember(index, response.then(({ bytes }) => bytes.subarray(offset, offset + BLOCK)))
      }
    }
    for (let index = first; index <= last; index += 1) {
      if (this.#blocks.has(index)) flush()
      else {
        run.push(index)
        if (run.length === BLOCKS_PER_REQUEST) flush()
      }
    }
    flush()
  }

  #block(index: number): Promise<Uint8Array> {
    const cached = this.#blocks.get(index)
    if (cached) {
      this.#blocks.delete(index)
      this.#blocks.set(index, cached)
      return cached
    }
    this.#fetchMissing(index, index)
    return this.#block(index)
  }

  #remember(index: number, block: Promise<Uint8Array>) {
    this.#blocks.set(index, block)
    block.catch(() => this.#blocks.delete(index))
    while (this.#blocks.size > CACHED_BLOCKS) {
      const oldest = this.#blocks.keys().next()
      if (oldest.done) break
      this.#blocks.delete(oldest.value)
    }
  }
}

async function fetchRange(itemId: string, start: number, end: number, signal: AbortSignal) {
  const response = await fetch(link("GET /api/item/{id}/book/file", { params: { id: itemId }, query: { start, end } }), { signal })
  if (!response.ok) throw await responseError(response)
  const total = Number(/\/(\d+)$/.exec(response.headers.get("Content-Range") ?? "")?.[1])
  const bytes = new Uint8Array(await response.arrayBuffer())
  if (response.status !== 206 || !Number.isSafeInteger(total) || bytes.length !== Math.min(end, total - 1) - start + 1) {
    throw new ApiError(`the book's bytes ${start}-${end} came back incomplete`, response.status, false, false)
  }
  return { bytes, total }
}
