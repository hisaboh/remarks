// @vitest-environment happy-dom

import type Content from '../../block/base/content';
import type { Muya } from '../../muya';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { Muya as MuyaClass } from '../../muya';
import { SelectionCaretType, SelectionDirection } from '../../selection/types';

// Whole-line paste (df174e68 / ea876148): when text/plain ends in a newline a
// COMPLETE line was copied, so pasting it at a collapsed caret keeps the line
// structure — the pasted line(s) become their own block(s) rather than being
// spliced inline (which would merge two lines into one and drop the break).
// Re-applied on top of upstream's paste re-audit (#4549), whose default treats
// `alpha\n` as inline text (`be|ta` -> `bealphata`).
//
// Driven through the real engine (the pre-#4549 harness mocked the old
// selection shape): paste a text/plain snapshot with a trailing newline at a
// stubbed caret and assert on the resulting markdown.

const bootedHosts: HTMLElement[] = [];
let hadVersion = false;
let originalVersion: string | undefined;

beforeEach(() => {
    hadVersion = 'MUYA_VERSION' in window;
    originalVersion = window.MUYA_VERSION;
    window.MUYA_VERSION = 'test';
});

afterEach(() => {
    while (bootedHosts.length) bootedHosts.pop()!.remove();
    if (hadVersion)
        window.MUYA_VERSION = originalVersion as string;
    else delete (window as Partial<Window>).MUYA_VERSION;
});

function bootMuya(markdown: string): Muya {
    const host = document.createElement('div');
    document.body.appendChild(host);
    const muya = new MuyaClass(host, { markdown } as ConstructorParameters<typeof MuyaClass>[1]);
    muya.init();
    bootedHosts.push(muya.domNode);
    return muya;
}

function firstBlock(muya: Muya): Content {
    return muya.editor.scrollPage!.firstContentInDescendant()!;
}

function stubSelection(muya: Muya, block: Content, offset: number) {
    const path = block.path;
    muya.editor.selection.getSelection = () => ({
        anchor: { offset, block, path },
        focus: { offset, block, path },
        isCollapsed: true,
        isSelectionInSameBlock: true,
        direction: SelectionDirection.FORWARD,
        type: SelectionCaretType.RANGE,
    });
}

// Paste a text/plain snapshot at a collapsed caret (offset) in `block`; returns
// the document markdown after the paste settles. No text/html flavor — the
// whole-line branch keys off the text/plain trailing newline.
async function paste(muya: Muya, block: Content, offset: number, text: string): Promise<string> {
    stubSelection(muya, block, offset);
    const event = {
        preventDefault() {},
        stopPropagation() {},
        clipboardData: {
            getData: (t: string) => (t === 'text/plain' ? text : ''),
            files: [],
            items: [],
        },
    } as unknown as ClipboardEvent;
    await muya.editor.clipboard.pasteHandler(event, text, '');
    await new Promise(r => setTimeout(r, 40));
    return muya.getMarkdown();
}

const NL = String.fromCharCode(10);

describe('clipboard.pasteHandler — whole-line paste', () => {
    it('inserts a whole line before the caret at the block start', async () => {
        const muya = bootMuya(`beta${NL}`);
        expect(await paste(muya, firstBlock(muya), 0, `alpha${NL}`)).toBe(`alpha${NL}${NL}beta${NL}`);
    });

    it('splits the line when the caret is mid-line (text-editor semantics)', async () => {
        const muya = bootMuya(`beta${NL}`);
        // be|ta + "alpha\n" -> "bealpha" / "ta"
        expect(await paste(muya, firstBlock(muya), 2, `alpha${NL}`)).toBe(`bealpha${NL}${NL}ta${NL}`);
    });

    it('appends a trailing empty line when pasting a whole line at the line end', async () => {
        const muya = bootMuya(`beta${NL}`);
        expect(await paste(muya, firstBlock(muya), 4, `alpha${NL}`)).toBe(`betaalpha${NL}${NL}`);
    });

    it('inserts multiple whole lines before the caret in order', async () => {
        const muya = bootMuya(`beta${NL}`);
        expect(await paste(muya, firstBlock(muya), 0, `a${NL}b${NL}`)).toBe(`a${NL}b${NL}${NL}beta${NL}`);
    });

    it('inserts the middle lines when pasting multiple whole lines mid-line', async () => {
        const muya = bootMuya(`beta${NL}`);
        expect(await paste(muya, firstBlock(muya), 2, `a${NL}b${NL}`)).toBe(`bea${NL}b${NL}${NL}ta${NL}`);
    });

    it('keeps an empty target line below the pasted line', async () => {
        const muya = bootMuya(NL);
        expect(await paste(muya, firstBlock(muya), 0, `alpha${NL}`)).toBe(`alpha${NL}${NL}`);
    });

    it('still merges text inline without a trailing newline', async () => {
        const muya = bootMuya(`beta${NL}`);
        // No trailing newline -> not a whole-line copy -> upstream inline merge.
        expect(await paste(muya, firstBlock(muya), 2, 'alpha')).toBe(`bealphata${NL}`);
    });

    it('does not apply inside code-block content (falls through to literal paste)', async () => {
        const muya = bootMuya(`\`\`\`${NL}code${NL}\`\`\`${NL}`);
        const codeContent = firstBlock(muya).nextContentInContext()!;
        expect(codeContent.blockName).toBe('codeblock.content');
        const md = await paste(muya, codeContent, 2, `alpha${NL}`);
        // Stays a single fenced code block — the whole-line block-split did not fire.
        expect(md.startsWith('```')).toBe(true);
        expect(md).toContain('alpha');
    });
});
