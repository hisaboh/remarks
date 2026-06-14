import * as pdfjsLib from 'pdfjs-dist';
// `?url` (Vite) yields the bundled worker's URL so PDF.js can spawn it.
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

export interface IPdfPageRender {
    dataUrl: string;
    width: number;
    height: number;
}

// Keyed by src; stores the in-flight/resolved render so concurrent callers
// (and re-renders) share a single rasterisation.
const renderCache = new Map<string, Promise<IPdfPageRender>>();

/**
 * Render the first page of a PDF to a PNG data URL.
 *
 * `<img>` cannot display a PDF, so embedded `![](x.pdf)` references are
 * rasterised here (ported from the legacy muyajs engine). PDF.js would
 * normally fetch the file itself, but the renderer's CSP `connect-src` blocks
 * `file://` fetches, so the bytes are read through the host file bridge
 * (`window.fileUtils.readFile`) and handed to PDF.js directly.
 *
 * The page is rendered at `baseScale × devicePixelRatio` so the PNG carries
 * enough pixels to stay sharp on HiDPI displays and when the editor is zoomed;
 * the consumer makes the container block-level (`mu-pdf-figure`) and lets CSS
 * scale the img down to the layout width.
 *
 * @param src `file://` URL of the PDF (as produced by `getImageSrc`).
 * @param baseScale base scale factor (1.5 ≈ 144dpi at dpr=1).
 */
export function loadPdfPage(src: string, baseScale = 1.5): Promise<IPdfPageRender> {
    const cached = renderCache.get(src);
    if (cached)
        return cached;

    const promise = (async (): Promise<IPdfPageRender> => {
        if (typeof window === 'undefined' || !window.fileUtils)
            throw new Error('loadPdfPage: no host file bridge available');

        const filePath = decodeURIComponent(src.replace(/^file:\/\//, ''));
        const bytes = await window.fileUtils.readFile(filePath);
        const data = typeof bytes === 'string' ? new TextEncoder().encode(bytes) : bytes;

        const pdf = await pdfjsLib.getDocument({ data }).promise;
        const page = await pdf.getPage(1);

        const dpr = window.devicePixelRatio || 1;
        const viewport = page.getViewport({ scale: baseScale * dpr });

        const canvas = document.createElement('canvas');
        canvas.width = viewport.width;
        canvas.height = viewport.height;
        const canvasContext = canvas.getContext('2d');
        if (!canvasContext)
            throw new Error('loadPdfPage: 2d canvas context unavailable');

        await page.render({ canvas, canvasContext, viewport }).promise;

        return {
            dataUrl: canvas.toDataURL('image/png'),
            width: viewport.width,
            height: viewport.height,
        };
    })();

    renderCache.set(src, promise);
    // Drop failures so a transient error doesn't permanently poison the cache.
    promise.catch(() => renderCache.delete(src));

    return promise;
}
