import * as pdfjsLib from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'

pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl

// Keyed by src; stores Promise<{dataUrl, width, height}> so concurrent callers
// share one render and do not race.
const renderCache = new Map()

/**
 * Render the first page of a PDF to a PNG data URL.
 *
 * PDF.js normally fetches the PDF itself, but CSP's connect-src blocks
 * file:// fetches from the renderer.  We read the bytes via the Electron
 * IPC bridge (window.fileUtils.readFile) and pass them directly so PDF.js
 * never makes a network request.
 *
 * @param {string} src  file:// URL of the PDF (as produced by getImageInfo)
 * @param {number} scale  device-pixel ratio for the canvas (default 1.5)
 * @returns {Promise<{dataUrl: string, width: number, height: number}>}
 */
export const loadPdfPage = (src, scale = 1.5) => {
  if (renderCache.has(src)) {
    return renderCache.get(src)
  }

  const promise = (async () => {
    // Convert file:// URL → plain filesystem path and undo %20 etc.
    const filePath = decodeURIComponent(src.replace(/^file:\/\//, ''))
    const data = await window.fileUtils.readFile(filePath)

    const pdf = await pdfjsLib.getDocument({ data }).promise
    const page = await pdf.getPage(1)
    const viewport = page.getViewport({ scale })

    const canvas = document.createElement('canvas')
    canvas.width = viewport.width
    canvas.height = viewport.height
    await page.render({ canvasContext: canvas.getContext('2d'), viewport }).promise

    return {
      dataUrl: canvas.toDataURL('image/png'),
      width: viewport.width,
      height: viewport.height
    }
  })()

  renderCache.set(src, promise)
  promise.catch(() => renderCache.delete(src))
  return promise
}
