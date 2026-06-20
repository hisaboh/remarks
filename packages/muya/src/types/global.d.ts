import type Content from '../block/base/content';
import type Parent from '../block/base/parent';

declare global {
    // eslint-disable-next-line ts/naming-convention
    interface Window {
        Prism: unknown;
        MUYA_VERSION: string;
        // Absolute directory of the document currently open in the host
        // (desktop) app. `getImageSrc` reads it to anchor relative local
        // image paths. Undefined in
        // non-desktop / headless contexts (the resolver then leaves relative
        // paths untouched rather than producing a broken `file://`).
        DIRNAME?: string;
        // Host (desktop) file bridge. `loadPdfPage` reads PDF bytes through it
        // so PDF.js never makes a (CSP-blocked) file:// fetch. Undefined in
        // non-desktop / headless contexts.
        fileUtils?: {
            readFile: (path: string, encoding?: string) => Promise<Uint8Array | string>;
        };
    }

    // eslint-disable-next-line ts/naming-convention
    interface Element {
        __MUYA_BLOCK__: Content | Parent;
    }

    // `Intl.Segmenter` (Stage 4, ES2022) is not in the ES2020 TS lib the
    // package targets. Declare the minimal surface we use so `visibleLength`
    // can call it without an `as any` escape hatch. The examples app
    // polyfills it when the runtime engine lacks support.
    namespace Intl {
        interface ISegmenterOptions {
            granularity?: 'grapheme' | 'word' | 'sentence';
        }
        interface ISegmentData {
            segment: string;
            index: number;
            input: string;
        }
        class Segmenter {
            constructor(locales?: string | string[], options?: ISegmenterOptions);
            segment(input: string): Iterable<ISegmentData>;
        }
    }
}
