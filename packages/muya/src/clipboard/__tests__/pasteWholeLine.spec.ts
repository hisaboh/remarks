import type Content from '../../block/base/content';
import type { Muya } from '../../muya';
import { describe, expect, it, vi } from 'vitest';

// The clipboard module pulls in CodeBlockContent → utils/prism which touches
// `window` at import time. Stub the prism shim so the test can run under Node
// (same stub as copyHandler.spec / getClipboardData.spec).
vi.mock('../../utils/prism/index', () => ({
    default: {},
    walkTokens: () => null,
    loadedLanguages: new Set(),
    transformAliasToOrigin: (s: string) => s,
    loadLanguage: () => null,
    search: () => [],
}));

// `normalizePastedHTML` needs a DOM (DOMPurify); identity is enough here.
vi.mock('../../utils/paste', async (importOriginal) => {
    const actual = await importOriginal<typeof import('../../utils/paste')>();
    return {
        ...actual,
        normalizePastedHTML: async (html: string) => html,
    };
});

// The whole-line branch instantiates real blocks via ScrollPage.loadBlock;
// substitute a recorder so the test can observe what would be inserted
// without building a live block tree.
vi.mock('../../block/scrollPage', () => ({
    ScrollPage: {
        loadBlock: (name: string) => ({
            create: (_muya: unknown, state: unknown) => ({
                name,
                state,
                parent: null,
                firstContentInDescendant: () => ({ setCursor: () => {} }),
            }),
        }),
    },
}));

const Clipboard = (await import('../index')).default;

interface IInsertRecord {
    method: 'insertBefore' | 'insertAfter';
    state: unknown;
}

// Minimal anchor content block + wrapper whose parent records insertions the
// way `Parent.insertBefore`/`insertAfter` would (including adopting the new
// node so chained `target.parent` walks keep working).
function makeAnchorWithWrapper(initialText: string, cursor = 0, blockName = 'paragraph.content') {
    const inserted: IInsertRecord[] = [];
    const parent = {
        insertBefore: (newNode: { parent: unknown; state: unknown }) => {
            newNode.parent = parent;
            inserted.push({ method: 'insertBefore', state: newNode.state });
            return newNode;
        },
        insertAfter: (newNode: { parent: unknown; state: unknown }) => {
            newNode.parent = parent;
            inserted.push({ method: 'insertAfter', state: newNode.state });
            return newNode;
        },
    };
    const wrapperBlock = { parent, blockName: 'paragraph' };
    const block = {
        text: initialText,
        blockName,
        getCursor: () => ({
            start: { offset: cursor },
            end: { offset: cursor },
        }),
        setCursor: vi.fn(),
        getAnchor: () => wrapperBlock,
        update: vi.fn(),
    };
    return {
        anchorBlock: block as unknown as Content & {
            setCursor: ReturnType<typeof vi.fn>;
        },
        inserted,
    };
}

function makeClipboard(anchorBlock: Content) {
    const clipboard = new Clipboard({ options: {} } as unknown as Muya);
    Object.defineProperty(clipboard, 'selection', {
        get: () => ({
            getSelection: () => ({
                isSelectionInSameBlock: true,
                anchorBlock,
            }),
        }),
    });
    return clipboard;
}

function makePasteEvent(data: Record<string, string> = {}) {
    return {
        preventDefault: vi.fn(),
        stopPropagation: vi.fn(),
        clipboardData: {
            getData: (type: string) => data[type] ?? '',
            files: [],
            items: [],
        },
    } as unknown as ClipboardEvent;
}

// Whole-line paste semantics (carried over from the Remarks muyajs engine,
// commit f86286a3): text ending in a newline means complete line(s) were
// copied; pasted at the start of a block they become standalone block(s)
// before it instead of merging into its text.
describe('clipboard.pasteHandler — whole-line paste', () => {
    it('inserts "alpha\\n" as a block before the target instead of merging', async () => {
        const { anchorBlock, inserted } = makeAnchorWithWrapper('beta', 0);
        const clipboard = makeClipboard(anchorBlock);

        await clipboard.pasteHandler(makePasteEvent({ 'text/plain': 'alpha\n' }));

        expect(inserted).toHaveLength(1);
        expect(inserted[0].method).toBe('insertBefore');
        expect(inserted[0].state).toMatchObject({ name: 'paragraph', text: 'alpha' });
        // The target line is untouched and keeps the caret at its start.
        expect(anchorBlock.text).toBe('beta');
        expect(anchorBlock.setCursor).toHaveBeenCalledWith(0, 0, true);
    });

    it('inserts multiple whole lines in order', async () => {
        const { anchorBlock, inserted } = makeAnchorWithWrapper('beta', 0);
        const clipboard = makeClipboard(anchorBlock);

        await clipboard.pasteHandler(makePasteEvent({ 'text/plain': 'one\n\ntwo\n' }));

        expect(inserted).toHaveLength(2);
        expect(inserted[0]).toMatchObject({
            method: 'insertBefore',
            state: { name: 'paragraph', text: 'one' },
        });
        expect(inserted[1]).toMatchObject({
            method: 'insertAfter',
            state: { name: 'paragraph', text: 'two' },
        });
        expect(anchorBlock.text).toBe('beta');
    });

    it('keeps an empty target line below the pasted line', async () => {
        const { anchorBlock, inserted } = makeAnchorWithWrapper('', 0);
        const clipboard = makeClipboard(anchorBlock);

        await clipboard.pasteHandler(makePasteEvent({ 'text/plain': 'alpha\n' }));

        expect(inserted).toHaveLength(1);
        // The empty paragraph is NOT consumed — the line break survives.
        expect(anchorBlock.text).toBe('');
        expect(anchorBlock.setCursor).toHaveBeenCalledWith(0, 0, true);
    });

    it('still merges text without a trailing newline', async () => {
        const { anchorBlock, inserted } = makeAnchorWithWrapper('beta', 0);
        const clipboard = makeClipboard(anchorBlock);

        await clipboard.pasteHandler(makePasteEvent({ 'text/plain': 'alpha' }));

        expect(inserted).toHaveLength(0);
        expect(anchorBlock.text).toBe('alphabeta');
        expect(anchorBlock.setCursor).toHaveBeenCalledWith(5, 5, true);
    });

    it('splits the line when the caret is mid-line (text-editor semantics)', async () => {
        const { anchorBlock, inserted } = makeAnchorWithWrapper('beta', 2);
        const clipboard = makeClipboard(anchorBlock);

        await clipboard.pasteHandler(makePasteEvent({ 'text/plain': 'alpha\n' }));

        // "be|ta" + "alpha\n" → "bealpha" / "ta" — the first pasted line
        // merges into the front half, the back half becomes its own line.
        expect(anchorBlock.text).toBe('bealpha');
        expect(inserted).toHaveLength(1);
        expect(inserted[0]).toMatchObject({
            method: 'insertAfter',
            state: { name: 'paragraph', text: 'ta' },
        });
    });

    it('appends a trailing empty line when pasting a whole line at the line end', async () => {
        const { anchorBlock, inserted } = makeAnchorWithWrapper('beta', 4);
        const clipboard = makeClipboard(anchorBlock);

        await clipboard.pasteHandler(makePasteEvent({ 'text/plain': 'alpha\n' }));

        expect(anchorBlock.text).toBe('betaalpha');
        expect(inserted).toHaveLength(1);
        expect(inserted[0]).toMatchObject({
            method: 'insertAfter',
            state: { name: 'paragraph', text: '' },
        });
    });

    it('inserts the middle lines when pasting multiple whole lines mid-line', async () => {
        const { anchorBlock, inserted } = makeAnchorWithWrapper('beta', 2);
        const clipboard = makeClipboard(anchorBlock);

        await clipboard.pasteHandler(makePasteEvent({ 'text/plain': 'one\n\ntwo\n' }));

        expect(anchorBlock.text).toBe('beone');
        expect(inserted).toHaveLength(2);
        expect(inserted[0]).toMatchObject({
            method: 'insertAfter',
            state: { name: 'paragraph', text: 'two' },
        });
        expect(inserted[1]).toMatchObject({
            method: 'insertAfter',
            state: { name: 'paragraph', text: 'ta' },
        });
    });

    it('does not apply inside code block content', async () => {
        const { anchorBlock, inserted } = makeAnchorWithWrapper(
            'const a = 1',
            0,
            'codeblock.content',
        );
        const clipboard = makeClipboard(anchorBlock);

        await clipboard.pasteHandler(makePasteEvent({ 'text/plain': 'alpha\n' }));

        expect(inserted).toHaveLength(0);
        expect(anchorBlock.text).toBe('alpha\nconst a = 1');
    });
});
