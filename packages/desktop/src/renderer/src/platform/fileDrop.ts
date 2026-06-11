// Open files/folders dropped onto the window.
//
// Under Tauri (dragDropEnabled, the default) the webview gets NO HTML drag/drop
// events and File objects carry no path — Tauri delivers the native paths via
// onDragDropEvent instead. So the renderer's HTML drop flow (webUtils
// getPathForFile → mt::window::drop) can't work; we handle the native event and
// route through the existing open-file / open-folder commands.

import { getCurrentWebview } from '@tauri-apps/api/webview'
import bus from '@/bus'
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

// Image extensions muya accepts (mirrors its IMAGE_EXT_REG).
const IMAGE_EXTS = new Set(['jpeg', 'jpg', 'png', 'gif', 'svg', 'webp'])

const isImage = (path: string): boolean => {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  return path.includes('.') && IMAGE_EXTS.has(ext)
}

const handleDrop = async(paths: string[]): Promise<void> => {
  for (const path of paths) {
    if (isMarkdown(path)) {
      send('mt::open-file', path)
    } else if (isImage(path)) {
      // @muyajs/core's own drag-drop image flow needs HTML drag events,
      // which Tauri's native drag handling suppresses — route the dropped
      // image into the editor at the cursor via the same bus event the
      // menu/sidebar insert-image path uses.
      bus.emit('insert-image', path)
    } else if (await invoke('mt::fs::is-directory', path)) {
      // Dropped a folder → open it as the sidebar project.
      send('mt::open-folder-path', path)
    }
    // Other non-markdown files are ignored for now.
  }
}

export const installFileDrop = (): void => {
  getCurrentWebview()
    .onDragDropEvent((event) => {
      if (event.payload.type !== 'drop') return
      handleDrop(event.payload.paths).catch((err) => {
        console.error('[platform] file drop handling failed:', err)
      })
    })
    .catch((err) => {
      console.error('[platform] failed to install file drop listener:', err)
    })
}
