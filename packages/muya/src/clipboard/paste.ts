import type Content from '../block/base/content';
import type Parent from '../block/base/parent';
import type TreeNode from '../block/base/treeNode';
import type { Nullable } from '../types';
import type Clipboard from './index';
import CodeBlockContent from '../block/content/codeBlockContent';
import { ScrollPage } from '../block/scrollPage';
import { URL_REG } from '../config';
import HtmlToMarkdown from '../state/htmlToMarkdown';
import { MarkdownToState } from '../state/markdownToState';
import { isParagraphState } from '../state/types';
import { getClipboardImageFile, getCopyTextType, isStandaloneTableHtml, normalizePastedHTML } from '../utils/paste';
import { mergePasteIntoHeading } from './mergePasteIntoHeading';
import { tryPasteImage } from './pasteImage';
import { PasteType } from './types';

// Everything the per-anchor paste handlers need from the synchronous snapshot
// taken before any block mutation: the target leaf, its wrapper block, and the
// current selection range.
interface IPasteContext {
    anchorBlock: Content;
    wrapperBlock: Nullable<Parent>;
    originWrapperBlock: Nullable<Parent>;
    start: { offset: number };
    end: { offset: number };
    content: string;
}

/**
 * Whether the frozen table-cell selection covers exactly one cell. Mirrors
 * the single-cell shape check used by the copy path: one row containing one
 * cell. Used to decide between replacing a single cell's text and cancelling
 * a multi-cell paste.
 */
function isSingleCellSelected(clipboard: Clipboard): boolean {
    const state = clipboard.selection.table.getStateForCopy();
    if (state == null)
        return false;

    return state.children.length === 1 && state.children[0].children.length === 1;
}

// Whole-line paste: a trailing newline in text/plain means complete line(s)
// were copied (`getClipboardData` serializes multi-block selections with their
// line terminator). With a collapsed caret in a paragraph/heading, insert the
// pasted line(s) as standalone block(s) instead of splicing into the anchor's
// inline text — plain-text editor semantics. Returns true when it handled the
// paste, false to fall through to `applyParsedPaste`.
//
// The signal is read from `text` (text/plain), NOT `markdown`: muya's own
// copies carry a text/html flavor too, and the HTML→Markdown conversion drops
// the trailing newline that text/plain faithfully keeps.
function applyWholeLinePaste(
    clipboard: Clipboard,
    ctx: IPasteContext,
    markdown: string,
    text: string,
): boolean {
    const { muya } = clipboard;
    const { anchorBlock, wrapperBlock, start, end, content } = ctx;

    const isCaretInTextBlock
        = /\n$/.test(text)
            && start.offset === end.offset
            && /^(?:paragraph\.content|atxheading\.content|setextheading\.content)$/.test(
                anchorBlock.blockName,
            );
    if (!isCaretInTextBlock)
        return false;

    const {
        footnote,
        math,
        isGitlabCompatibilityEnabled,
        trimUnnecessaryCodeBlockEmptyLines,
        frontMatter,
    } = muya.options;
    const states = new MarkdownToState({
        footnote,
        math,
        isGitlabCompatibilityEnabled,
        trimUnnecessaryCodeBlockEmptyLines,
        frontMatter,
    }).generate(markdown);

    if (states.length === 0)
        return false;

    if (start.offset === 0) {
        // Caret at the block start: pasted line(s) go before it; the anchor
        // (possibly empty) survives below, preserving the line break.
        let target: Nullable<Parent> = null;
        for (const state of states) {
            const newBlock = ScrollPage.loadBlock(state.name).create(muya, state);
            if (target == null)
                wrapperBlock?.parent?.insertBefore(newBlock, wrapperBlock);
            else
                target.parent?.insertAfter(newBlock, target);

            target = newBlock;
        }

        anchorBlock.setCursor(0, 0, true);

        return true;
    }

    // Caret mid-line (or at line end): split the line at the caret, merge the
    // first pasted line into the front half, insert the remaining line(s), and
    // the back half becomes its own line with the caret at its start.
    const pre = content.substring(0, start.offset);
    const post = content.substring(end.offset);

    let rest = states;
    const firstState = states[0];
    if (isParagraphState(firstState)) {
        anchorBlock.text = pre + firstState.text;
        rest = states.slice(1);
    }
    else {
        anchorBlock.text = pre;
    }
    anchorBlock.update();

    let target: Nullable<Parent> = wrapperBlock;
    for (const state of rest) {
        const newBlock = ScrollPage.loadBlock(state.name).create(muya, state);
        target?.parent?.insertAfter(newBlock, target);
        target = newBlock;
    }

    const postBlock = ScrollPage.loadBlock('paragraph').create(muya, {
        name: 'paragraph',
        text: post,
    });
    target?.parent?.insertAfter(postBlock, target);
    postBlock.firstContentInDescendant()?.setCursor(0, 0, true);

    return true;
}

// A single plain paragraph (no block structure) is inline text — merge it into
// the anchor's text at the caret/selection rather than inserting a new block.
// This restores the legacy muyajs / pre-refactor tauri behaviour where pasting
// e.g. "alpha" into "beta" gives "alphabeta". Structured single-line markdown
// (`# h`, `- x`, `> q`, a one-row table) parses to a non-paragraph state and is
// left to `applyParsedPaste`, which builds the real block. Returns true when it
// handled the paste.
function applyInlineParagraphPaste(
    clipboard: Clipboard,
    ctx: IPasteContext,
    markdown: string,
): boolean {
    const { muya } = clipboard;
    const { anchorBlock, start, end, content } = ctx;

    // Only intercept when the anchor already has text — pasting into a mid-text
    // caret/selection is the inline case. An empty anchor is a fresh-block
    // placeholder, so plain text there is left to `applyParsedPaste` (which
    // creates the paragraph block and is what develop's tests expect); the
    // visible result is identical either way.
    if (content === '')
        return false;

    const {
        footnote,
        math,
        isGitlabCompatibilityEnabled,
        trimUnnecessaryCodeBlockEmptyLines,
        frontMatter,
    } = muya.options;
    const states = new MarkdownToState({
        footnote,
        math,
        isGitlabCompatibilityEnabled,
        trimUnnecessaryCodeBlockEmptyLines,
        frontMatter,
    }).generate(markdown);

    const firstState = states[0];
    if (states.length !== 1 || !isParagraphState(firstState))
        return false;

    const insertText = firstState.text;
    anchorBlock.text
        = content.substring(0, start.offset) + insertText + content.substring(end.offset);
    anchorBlock.update();
    const offset = start.offset + insertText.length;
    anchorBlock.setCursor(offset, offset, true);

    return true;
}

// Parse a paste into real blocks (the common anchor case): parse markdown →
// state, splice a leading paragraph back into a heading anchor, drop the
// selected range, insert the new blocks, remove the emptied source paragraph,
// and seat the cursor at the end.
function applyParsedPaste(
    clipboard: Clipboard,
    ctx: IPasteContext,
    markdown: string,
): void {
    const { muya } = clipboard;
    const { anchorBlock, originWrapperBlock, start, end, content } = ctx;
    let wrapperBlock = ctx.wrapperBlock;

    // An empty / whitespace-only paste is a no-op; the parser would otherwise
    // emit a lone empty paragraph and churn blocks.
    if (markdown.trim().length === 0)
        return;

    const {
        footnote,
        math,
        isGitlabCompatibilityEnabled,
        trimUnnecessaryCodeBlockEmptyLines,
        frontMatter,
    } = muya.options;

    const states = new MarkdownToState({
        footnote,
        math,
        isGitlabCompatibilityEnabled,
        trimUnnecessaryCodeBlockEmptyLines,
        frontMatter,
    }).generate(markdown);

    // When pasting into a heading, splice the first paragraph back into the
    // heading text so the heading semantics survive. The helper also collapses
    // any selection on the heading.
    const remaining = mergePasteIntoHeading(
        anchorBlock,
        wrapperBlock,
        states,
        { startOffset: start.offset, endOffset: end.offset },
    );

    if (remaining === states && start.offset !== end.offset) {
        anchorBlock.text
            = content.substring(0, start.offset) + content.substring(end.offset);
        anchorBlock.update();
    }

    for (const state of remaining) {
        const newBlock = ScrollPage.loadBlock(state.name).create(muya, state);
        wrapperBlock?.parent?.insertAfter(newBlock, wrapperBlock);
        wrapperBlock = newBlock;
    }

    // Remove empty paragraph when paste.
    if (originWrapperBlock?.blockName === 'paragraph') {
        const originState = originWrapperBlock.getState();
        if (isParagraphState(originState) && originState.text === '')
            originWrapperBlock.remove();
    }

    const cursorBlock = wrapperBlock?.firstContentInDescendant();
    const offset = cursorBlock?.text.length;

    if (offset != null)
        cursorBlock?.setCursor(offset, offset, true);
}

// `language-input`, `table.cell.content` and `codeblock.content` never parse a
// paste into blocks — they take the text literally.
function applyLiteralPaste(
    clipboard: Clipboard,
    ctx: IPasteContext,
    initialMarkdown: string,
): void {
    const { anchorBlock, start, end, content } = ctx;
    let markdown = initialMarkdown;

    // A frozen table-cell selection scopes the paste: a single cell gets its
    // text replaced (with `\n` → `<br/>`); a multi-cell rectangle cancels the
    // paste.
    if (
        anchorBlock.blockName === 'table.cell.content'
        && clipboard.selection.table.hasSelection
    ) {
        if (!isSingleCellSelected(clipboard))
            return;

        anchorBlock.text = markdown.trim().replace(/\n/g, '<br/>');
        const offset = anchorBlock.text.length;
        anchorBlock.setCursor(offset, offset, true);
        clipboard.selection.table.clear();

        return;
    }

    if (anchorBlock.blockName === 'language-input')
        markdown = markdown.replace(/\n/g, '');
    else if (anchorBlock.blockName === 'table.cell.content')
        markdown = markdown.replace(/\n/g, '<br/>');

    anchorBlock.text
        = content.substring(0, start.offset)
            + markdown
            + content.substring(end.offset);
    const offset = start.offset + markdown.length;
    anchorBlock.setCursor(offset, offset, true);
    // Update html preview if the out container is `html-block`
    if (
        anchorBlock instanceof CodeBlockContent
        && anchorBlock.outContainer
        && /html-block|math-block|diagram/.test(
            anchorBlock.outContainer.blockName,
        )
    ) {
        // The attachments list of html-block / math-block / diagram blocks
        // always opens with the render preview node, which exposes an
        // `update(text)` method. The LinkedList itself is typed loosely;
        // narrow via a structural shape check before calling.
        const head = anchorBlock.outContainer.attachments.head;
        const updater = head as TreeNode & { update?: (text: string) => void };
        if (typeof updater.update === 'function')
            updater.update(anchorBlock.text);
    }
}

// Block-level HTML (`<ul>`/`<ol>`/`<pre>`/`<blockquote>` … — tags in
// `PARAGRAPH_TYPES`) lands as a live html-block, not a fenced ```html code
// block, so the markup renders in place.
function applyHtmlBlockPaste(
    clipboard: Clipboard,
    ctx: IPasteContext,
    text: string,
): void {
    const { muya } = clipboard;
    const { wrapperBlock, originWrapperBlock } = ctx;
    const state = {
        name: 'html-block',
        text: text.trim(),
    };
    const newBlock = ScrollPage.loadBlock(state.name).create(muya, state);
    wrapperBlock?.parent?.insertAfter(newBlock, wrapperBlock);

    // Drop the empty paragraph the html-block replaced.
    if (originWrapperBlock?.blockName === 'paragraph') {
        const originState = originWrapperBlock.getState();
        if (isParagraphState(originState) && originState.text === '')
            originWrapperBlock.remove();
    }

    const offset = state.text.length;
    newBlock.lastContentInDescendant().setCursor(offset, offset, true);
}

// Everything the paste pipeline needs, snapshotted up front so it survives the
// async hops (image hook, HTML normalization) without re-reading a possibly
// detached clipboard.
interface IPasteData {
    text: string;
    html: string;
    imageFile: File | null;
    pasteType: PasteType;
}

// The paste pipeline, decoupled from the DOM `paste` event so it can be driven
// either by a trusted paste event (`pasteSelection`) or by an explicit
// clipboard read (`pastePlainText`). The latter exists because Chromium removed
// programmatic clipboard reads via `document.execCommand('paste')`, so the
// "set a flag → execCommand('paste') → handle the synthetic event" approach no
// longer fires any paste event at all.
async function applyPaste(clipboard: Clipboard, data: IPasteData): Promise<void> {
    const { muya } = clipboard;
    const { bulletListMarker } = muya.options;
    const selection = clipboard.selection.getSelection();
    if (!selection)
        return;

    const { isSelectionInSameBlock, anchorBlock } = selection;

    if (!anchorBlock)
        return;

    const { text, imageFile, pasteType } = data;
    let { html } = data;

    if (!isSelectionInSameBlock) {
        clipboard.cutHandler();

        return applyPaste(clipboard, data);
    }

    // When the clipboard holds an image — either a file resolved to a path
    // or an in-memory bitmap — insert it as an inline image
    // routed through `imageAction`, short-circuiting the text/HTML paste.
    if (await tryPasteImage(clipboard, anchorBlock, imageFile))
        return;

    // Support pasted URLs from Firefox.
    if (URL_REG.test(text) && !/\s/.test(text) && !html)
        html = `<a href="${text}">${text}</a>`;

    // Apple Numbers and a handful of other sources only put a raw
    // `<table>...</table>` blob in text/plain. Promote it to the HTML
    // slot so it goes through the HTML→Markdown converter rather than
    // being inserted verbatim.
    if (!html && isStandaloneTableHtml(text))
        html = text;

    // Remove crap from HTML such as meta data and styles.
    html = await normalizePastedHTML(html);
    const copyType = getCopyTextType(html, text, pasteType);

    const { start, end } = anchorBlock.getCursor()!;
    const { text: content } = anchorBlock;
    const wrapperBlock = anchorBlock.getAnchor();
    const ctx: IPasteContext = {
        anchorBlock,
        wrapperBlock,
        originWrapperBlock: wrapperBlock,
        start,
        end,
        content,
    };

    if (/html|text/.test(copyType)) {
        const markdown
            = copyType === 'html' && anchorBlock.blockName !== 'codeblock.content'
                ? new HtmlToMarkdown({ bulletListMarker }).generate(html)
                : text;

        // Every non-literal anchor always parses through `MarkdownToState`,
        // regardless of line count, so a single line of `# heading` / `- list`
        // / a one-row table becomes real structure.
        const isLiteralAnchor
            = anchorBlock.blockName === 'language-input'
                || anchorBlock.blockName === 'table.cell.content'
                || anchorBlock.blockName === 'codeblock.content';

        if (!isLiteralAnchor) {
            // Whole-line paste (#7 era / tauri2.0 feature): when complete
            // line(s) were copied (text/plain ends in a newline) and the caret
            // is collapsed in a paragraph/heading, insert them as standalone
            // blocks rather than splicing into the anchor's inline text.
            if (applyWholeLinePaste(clipboard, ctx, markdown, text)) {
                // handled
            }
            // A single plain paragraph is inline text — merge it into the
            // anchor's text instead of inserting a new block (tauri2.0 / legacy
            // muyajs behaviour). Structured single-line markdown still becomes a
            // block via `applyParsedPaste`.
            else if (!applyInlineParagraphPaste(clipboard, ctx, markdown)) {
                applyParsedPaste(clipboard, ctx, markdown);
            }
        }
        else {
            applyLiteralPaste(clipboard, ctx, markdown);
        }
    }
    else {
        applyHtmlBlockPaste(clipboard, ctx, text);
    }
}

// Entry for a trusted DOM `paste` event (native Cmd/Ctrl+V).
export function pasteSelection(
    clipboard: Clipboard,
    event: ClipboardEvent,
    // `event.clipboardData` is only valid synchronously while the paste event
    // is being dispatched. Once the pipeline yields at its first `await`, the
    // browser may detach the DataTransfer and subsequent `getData()` calls
    // return ''. We snapshot text/html/image synchronously below; the snapshot
    // is then threaded through the `!isSelectionInSameBlock` recursion inside
    // `applyPaste` rather than re-reading the (now possibly detached) clipboard.
    rawText?: string,
    rawHtml?: string,
): Promise<void> {
    event.preventDefault();
    event.stopPropagation();

    if (!event.clipboardData)
        return Promise.resolve();

    const text = rawText ?? event.clipboardData.getData('text/plain');
    const html = rawHtml ?? event.clipboardData.getData('text/html');
    // Snapshot any in-memory image File (the bitmap / "Copy Image" /
    // screenshot case) synchronously too — `clipboardData.files` is also
    // detached after the first `await`.
    const imageFile = getClipboardImageFile(event.clipboardData);

    return applyPaste(clipboard, { text, html, imageFile, pasteType: clipboard.pasteType });
}

// Entry for "Paste as Plain Text". The caller has already read the clipboard's
// plain text (Chromium no longer fires a paste event for
// `document.execCommand('paste')`), so feed it straight into the pipeline with
// the plain-text flag and no HTML — the text is treated as markdown source
// rather than being synthesized from rich HTML.
export function pastePlainText(clipboard: Clipboard, text: string): Promise<void> {
    return applyPaste(clipboard, {
        text,
        html: '',
        imageFile: null,
        pasteType: PasteType.PASTE_AS_PLAIN_TEXT,
    });
}
