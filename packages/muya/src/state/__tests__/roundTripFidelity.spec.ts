import { describe, expect, it } from 'vitest';
import { MarkdownToState } from '../markdownToState';
import StateToMarkdown from '../stateToMarkdown';

const FENCE = '```';

function toState(markdown: string, preserveEmptyLines = true) {
    return new MarkdownToState({
        footnote: false,
        math: true,
        isGitlabCompatibilityEnabled: true,
        trimUnnecessaryCodeBlockEmptyLines: false,
        frontMatter: true,
        preserveEmptyLines,
    }).generate(markdown);
}

function roundTrip(markdown: string) {
    return new StateToMarkdown().generate(toState(markdown));
}

// Source-mode → WYSIWYG → source-mode must not lose authored content
// (user-reported: blank-line runs collapsed, fence info attributes truncated).
describe('markdown round-trip fidelity', () => {
    it('keeps an authored empty line between paragraphs', () => {
        const md = 'p1\n\n\np2\n';
        const states = toState(md);
        expect(states.map(s => s.name)).toEqual(['paragraph', 'paragraph', 'paragraph']);
        expect(roundTrip(md)).toBe(md);
    });

    it('keeps multiple authored empty lines and stays stable across round trips', () => {
        const md = 'p1\n\n\n\n\np2\n';
        const once = roundTrip(md);
        expect(once).toBe(md);
        expect(roundTrip(once)).toBe(once);
    });

    it('drops blank-line runs without preserveEmptyLines (CommonMark behavior)', () => {
        const states = toState('p1\n\n\n\np2\n', false);
        expect(states.map(s => s.name)).toEqual(['paragraph', 'paragraph']);
    });

    it('a plain paragraph separator adds no empty paragraphs', () => {
        const states = toState('p1\n\np2\n');
        expect(states.map(s => s.name)).toEqual(['paragraph', 'paragraph']);
    });

    // marked folds the blank-line run AFTER a heading into the heading token's
    // `raw` (no separate `space` token), so the empty lines must be recovered
    // in the heading handler — otherwise authored blank lines between/after
    // headings collapse on round trip.
    it('keeps an authored empty line between two headings', () => {
        const md = '# A\n\n\n# B\n';
        const states = toState(md);
        expect(states.map(s => s.name)).toEqual(['atx-heading', 'paragraph', 'atx-heading']);
        expect(roundTrip(md)).toBe(md);
    });

    it('keeps multiple authored empty lines between headings', () => {
        const md = '# A\n\n\n\n# B\n';
        expect(roundTrip(md)).toBe(md);
    });

    it('keeps an authored empty line between a heading and a paragraph', () => {
        const md = '# H\n\n\ntext\n';
        expect(roundTrip(md)).toBe(md);
    });

    it('a plain heading separator adds no empty paragraphs', () => {
        const states = toState('# A\n\n# B\n');
        expect(states.map(s => s.name)).toEqual(['atx-heading', 'atx-heading']);
    });

    it('drops blank lines after a heading without preserveEmptyLines', () => {
        const states = toState('# A\n\n\n# B\n', false);
        expect(states.map(s => s.name)).toEqual(['atx-heading', 'atx-heading']);
    });

    it('keeps the full fence info string (pandoc attributes)', () => {
        const md = `${FENCE}{#lst:peano_axioms .haskell caption="自然数の定義"}\ndata N = Zero | S N\n${FENCE}\n`;
        const states = toState(md);
        expect(states[0]).toMatchObject({
            name: 'code-block',
            meta: {
                type: 'fenced',
                lang: '{#lst:peano_axioms',
                info: '{#lst:peano_axioms .haskell caption="自然数の定義"}',
            },
        });
        expect(roundTrip(md)).toBe(md);
    });

    it('a bare language stores no info and round-trips unchanged', () => {
        const md = `${FENCE}haskell\ncode\n${FENCE}\n`;
        const states = toState(md);
        expect(states[0]).toMatchObject({
            name: 'code-block',
            meta: { type: 'fenced', lang: 'haskell' },
        });
        expect((states[0] as { meta: { info?: string } }).meta.info).toBeUndefined();
        expect(roundTrip(md)).toBe(md);
    });
});
