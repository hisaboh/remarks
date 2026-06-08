// Window dragging for `data-tauri-drag-region` elements.
//
// Tauri injects its own drag.js (a bubble-phase document mousedown listener),
// but it never fires here — the app stops mousedown propagation before it
// reaches `document`. So we replicate drag.js's region detection and call the
// window API ourselves, on the CAPTURE phase, so it runs before any app handler.

import { getCurrentWindow } from '@tauri-apps/api/window'

const DRAG_ATTR = 'data-tauri-drag-region'
const CLICKABLE_TAGS = new Set(['A', 'BUTTON', 'INPUT', 'SELECT', 'TEXTAREA', 'LABEL', 'SUMMARY'])
const INTERACTIVE_ROLES = new Set([
  'button',
  'link',
  'menuitem',
  'tab',
  'checkbox',
  'radio',
  'switch',
  'option'
])

const isClickable = (el: HTMLElement): boolean =>
  CLICKABLE_TAGS.has(el.tagName) ||
  (el.hasAttribute('contenteditable') && el.getAttribute('contenteditable') !== 'false') ||
  (el.hasAttribute('tabindex') && el.getAttribute('tabindex') !== '-1') ||
  INTERACTIVE_ROLES.has(el.getAttribute('role') ?? '')

// Mirrors Tauri drag.js isDragRegion: bare/"true" = only direct hits on the
// attributed element; "deep" = any descendant; "false" = blocked; clickable
// elements without the attr block dragging.
const isDragRegion = (path: EventTarget[]): boolean => {
  for (const node of path) {
    if (!(node instanceof HTMLElement)) continue
    const attr = node.getAttribute(DRAG_ATTR)
    if (isClickable(node) && attr === null) return false
    if (attr === null) continue
    if (attr === 'false') return false
    if (attr === 'deep') return true
    if (attr === '' || attr === 'true') return node === path[0]
  }
  return false
}

export const installDragRegion = (): void => {
  document.addEventListener(
    'mousedown',
    (e) => {
      if (e.button !== 0 || e.detail !== 1) return
      if (!isDragRegion(e.composedPath())) return
      e.preventDefault()
      void getCurrentWindow().startDragging()
    },
    true
  )
}
