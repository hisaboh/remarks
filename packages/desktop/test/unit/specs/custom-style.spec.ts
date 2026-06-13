import { afterEach, beforeAll, describe, expect, it } from 'vitest'

// Regression: editing the custom-theme CSS in preferences down to empty must
// clear the previously-applied styles. addCustomStyle used to `return` early
// when the new value was falsy, so removed/cleared custom CSS was never
// reflected — the stale `#custom-styles` content stayed live.
//
// `@/util/theme` reads `window.path.sep` at import time (via ../config), so
// stub it before a dynamic import (the i18n.spec pattern).
let addCustomStyle: (options: { customCss?: string }) => void

beforeAll(async() => {
  ;(window as unknown as { path: { sep: string } }).path = { sep: '/' }
  ;({ addCustomStyle } = await import('@/util/theme'))
})

describe('addCustomStyle', () => {
  const styleEl = () => document.querySelector('#custom-styles') as HTMLStyleElement | null

  afterEach(() => {
    styleEl()?.remove()
  })

  it('applies non-empty custom CSS into a #custom-styles element', () => {
    addCustomStyle({ customCss: 'p { color: red; }' })
    expect(styleEl()).not.toBeNull()
    expect(styleEl()?.innerHTML).toBe('p { color: red; }')
  })

  it('clears previously-applied CSS when the field is emptied', () => {
    addCustomStyle({ customCss: 'p { color: red; }' })
    expect(styleEl()?.innerHTML).toBe('p { color: red; }')

    // Editing the textarea down to empty.
    addCustomStyle({ customCss: '' })
    expect(styleEl()?.innerHTML).toBe('')
  })

  it('replaces the CSS when edited to a new value', () => {
    addCustomStyle({ customCss: 'p { color: red; }' })
    addCustomStyle({ customCss: 'a { color: blue; }' })
    expect(styleEl()?.innerHTML).toBe('a { color: blue; }')
  })

  it('is a no-op when there is no prior element and nothing to apply', () => {
    addCustomStyle({ customCss: '' })
    expect(styleEl()).toBeNull()
  })

  it('keeps #custom-styles last in <head> so user CSS wins the cascade', () => {
    const theme = document.createElement('style')
    theme.id = 'theme-style'
    document.head.appendChild(theme)

    addCustomStyle({ customCss: 'p { color: red; }' })
    expect(document.head.lastElementChild).toBe(styleEl())

    theme.remove()
  })
})
