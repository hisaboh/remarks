// Open files/folders dropped onto the window.
//
// Under Tauri (dragDropEnabled, the default) the webview gets NO HTML drag/drop
// events and File objects carry no path — Tauri delivers the native paths via
// onDragDropEvent instead. So the renderer's HTML drop flow (webUtils
// getPathForFile → mt::window::drop) can't work; we handle the native event and
// route through the existing open-file / open-folder commands.

import { getCurrentWebview } from '@tauri-apps/api/webview'
import { send, invoke } from './ipc'

// MarkText's openable text extensions (mirrors common/filesystem/paths.ts).
const MARKDOWN_EXTS = new Set([
  'markdown',
  'mdown',
  'mkdn',
  'md',
  'mkd',
  'mdwn',
  'mdtxt',
  'mdtext',
  'mdx',
  'text',
  'txt'
])

const isMarkdown = (path: string): boolean => {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  return path.includes('.') && MARKDOWN_EXTS.has(ext)
}

export const installFileDrop = (): void => {
  void getCurrentWebview().onDragDropEvent(async (event) => {
    if (event.payload.type !== 'drop') return
    for (const path of event.payload.paths) {
      if (isMarkdown(path)) {
        send('mt::open-file', path)
      } else if (await invoke('mt::fs::is-directory', path)) {
        // Dropped a folder → open it as the sidebar project.
        send('mt::open-folder-path', path)
      }
      // Other files (images, non-markdown) are ignored for now.
    }
  })
}
