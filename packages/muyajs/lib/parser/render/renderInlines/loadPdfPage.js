import * as pdfjsLib from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'

pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl

// Keyed by src; stores Promise<result> so concurrent callers share one render.
const renderCache = new Map()

/**
 * Render the first page of a PDF to a PNG data URL.
 *
 * PDF.js normally fetches the PDF itself, but CSP's connect-src blocks
 * file:// fetches from the renderer.  We read the bytes via the Electron
 * IPC bridge (window.fileUtils.readFile) and pass them directly so PDF.js
 * never makes a network request.
 *
 * The canvas is rendered at baseScale × devicePixelRatio so that the PNG
 * data URL contains enough pixels to stay sharp on Retina displays and when
 * the editor is zoomed in.  The caller is expected to add the `ag-pdf-figure`
 * class to the outer .ag-inline-image container (making it block-level) and
 * apply `width: 100%; height: auto` to the img via CSS, so the high-res pixels
 * are used for sharpness rather than for layout width.
 *
 * @param {string} src  file:// URL of the PDF (as produced by getImageInfo)
 * @param {number} baseScale  base scale factor (default 1.5 ≈ 144 dpi at dpr=1)
 * @returns {Promise<{dataUrl: string, width: number, height: number}>}
 */
export const loadPdfPage = (src, baseScale = 1.5) => {
  if (renderCache.has(src)) {
    return renderCache.get(src)
  }

  const promise = (async () => {
    const filePath = decodeURIComponent(src.replace(/^file:\/\//, ''))
    const data = await window.fileUtils.readFile(filePath)

    const pdf = await pdfjsLib.getDocument({ data }).promise
    const page = await pdf.getPage(1)

    // Render at physical-pixel resolution so the result stays sharp on
    // HiDPI screens and when the user zooms the editor view.
    const dpr = window.devicePixelRatio || 1
    const renderScale = baseScale * dpr
    const viewport = page.getViewport({ scale: renderScale })

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
