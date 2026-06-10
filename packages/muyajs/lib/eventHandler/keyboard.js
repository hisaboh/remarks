import { CLASS_OR_ID, EVENT_KEYS, KEYS_TO_IGNORE } from '../config'
import selection from '../selection'
import { findNearestParagraph, getTextContent } from '../selection/dom'
import { getParagraphReference, getImageInfo } from '../utils'
import { checkEditEmoji } from '../ui/emojis'

// True for keydown events that belong to an IME composition, including the
// commit key. WebKit reports keyCode 229 (the "IME processing" sentinel) for
// these — and crucially the commit Enter still has keyCode 229 even after
// isComposing/isComposed have flipped to false, so neither flag alone catches
// it. Both key handlers must ignore these, or the IME-commit Enter is processed
// as a paragraph split and corrupts the just-committed text.
const isImeKey = (event) => event.isComposing || event.keyCode === 229

class Keyboard {
  constructor(muya) {
    this.muya = muya
    this.isComposed = false
    // The IME composition being typed: target block key recorded at
    // compositionstart, committed exactly once after the composition ends.
    this.composition = null
    this.shownFloat = {}
    this.recordIsComposed()
    this.dispatchEditorState()
    this.keydownBinding()
    this.keyupBinding()
    this.beforeinputBinding()
    this.inputBinding()
    this.listen()
  }

  listen() {
    // cache shown float box
    this.muya.eventCenter.subscribe('muya-float', (tool, status) => {
      // We should use tool.name here instead as Vue3's reactivity since objects are stored as Proxy objects
      // This can cause reference issues if we use the original implementation of comparing via references.

      if (status) this.shownFloat[tool.name] = tool
      else delete this.shownFloat[tool.name]

      if (tool.name === 'ag-front-menu' && !status) {
        const seletedParagraph = this.muya.container.querySelector('.ag-selected')
        if (seletedParagraph) {
          this.muya.contentState.selectedBlock = null
          // prevent rerender, so change the class manually.
          seletedParagraph.classList.toggle('ag-selected')
        }
      }
    })
  }

  hideAllFloatTools() {
    for (const tool in this.shownFloat) {
      this.shownFloat[tool].hide()
    }
  }

  recordIsComposed() {
    const { container, eventCenter, contentState } = this.muya
    const handler = (event) => {
      if (event.type === 'compositionstart') {
        this.isComposed = true
        // Record the composition target now: the selection is still reliable
        // here, while at commit time WebKit may report a transiently invalid
        // or relocated selection. The commit resolves the block by this key.
        const { start } = selection.getCursorRange()
        const target = start || (contentState.cursor && contentState.cursor.start)
        this.composition = target
          ? { key: target.key, data: '', committed: false }
          : null
      } else if (event.type === 'compositionend') {
        this.isComposed = false
        // A second compositionend (WebKit fires it twice in some flows) can
        // carry empty data — keep the real committed string.
        if (this.composition && event.data) {
          this.composition.data = event.data
        }
        // Commit exactly once, after the DOM is final. WebKit fires the final
        // `input` (insertFromComposition) AFTER compositionend, Chromium
        // BEFORE it (tagged isComposing) — deferring one tick works for both
        // orders, and commitComposition dedups via the `committed` flag.
        const pending = this.composition
        setTimeout(() => this.commitComposition(event, pending))
      }
    }

    eventCenter.attachDOMEvent(container, 'compositionend', handler)
    // eventCenter.attachDOMEvent(container, 'compositionupdate', handler)
    eventCenter.attachDOMEvent(container, 'compositionstart', handler)
  }

  // Commit an ended IME composition into the model, exactly once. The target
  // block comes from the key recorded at compositionstart — never from the
  // live selection, which WebKit may have moved or invalidated by now.
  commitComposition(event, composition = this.composition) {
    const { contentState, eventCenter } = this.muya
    if (!composition || composition.committed) {
      return
    }
    // A new composition is already in progress (continuous clause input):
    // its own commit will pick up the final DOM text of the paragraph, and
    // touching the model now would cancel it.
    if (this.isComposed) {
      return
    }
    composition.committed = true
    if (composition === this.composition) {
      this.composition = null
    }

    const block = contentState.getBlock(composition.key)
    if (!block) {
      return
    }
    const paragraph = document.querySelector(`#${composition.key}`)
    if (!paragraph) {
      // WebKit's IME commit (deleteCompositionText → insertFromComposition)
      // DESTROYS the content span when the composition was its only content
      // (i.e. composing in an empty paragraph): the committed text lands
      // directly under the outer block element and every #key lookup fails.
      // Re-read the text from the outer block's DOM, then re-render it to
      // rebuild the proper span structure (singleRender patches against
      // toVNode(live DOM), so the out-of-band mutation is handled cleanly).
      const outer = contentState.getParent(block)
      const outerDom = outer && document.querySelector(`#${outer.key}`)
      if (!outerDom) {
        return
      }
      block.text = getTextContent(outerDom, [
        CLASS_OR_ID.AG_MATH_RENDER,
        CLASS_OR_ID.AG_RUBY_RENDER
      ])
      // Restore the caret to the end of the committed text — but only when
      // the selection is still inside this paragraph; after a click-away
      // commit it belongs wherever the user clicked.
      const liveSel = document.getSelection()
      if (liveSel && liveSel.anchorNode && outerDom.contains(liveSel.anchorNode)) {
        const offset = block.text.length
        contentState.cursor = {
          start: { key: composition.key, offset },
          end: { key: composition.key, offset }
        }
      }
      contentState.singleRender(outer)
      eventCenter.dispatch('stateChange')
      return
    }

    const sel = document.getSelection()
    const anchor = sel ? sel.anchorNode : null
    const anchorElement = anchor && anchor.nodeType === 3 ? anchor.parentNode : anchor
    const inTarget =
      anchorElement &&
      typeof anchorElement.closest === 'function' &&
      anchorElement.closest('.ag-paragraph') === paragraph

    if (inTarget) {
      contentState.inputHandler({
        type: 'compositionend',
        data: composition.data || (event && event.data) || ''
      })
      this.checkLanguageInput()
    } else {
      // The selection already left the paragraph (click-away commit). Don't
      // move it back — just sync the model to the committed DOM text so the
      // next model-driven render doesn't wipe it.
      block.text = getTextContent(paragraph, [
        CLASS_OR_ID.AG_MATH_RENDER,
        CLASS_OR_ID.AG_RUBY_RENDER
      ])
    }
    // `stateChange` subscribers call muya.dispatchChange, so don't dispatch both.
    eventCenter.dispatch('stateChange')
  }

  dispatchEditorState() {
    const { container, eventCenter } = this.muya

    let timer = null
    const changeHandler = (event) => {
      // Never touch the selection around an IME composition: getCursorRange's
      // needFix path calls setCursorRange → removeAllRanges, which cancels an
      // in-progress composition and loses the typed text.
      if (this.isComposed || isImeKey(event)) {
        return
      }
      if (
        event.type === 'keyup' &&
        (event.key === EVENT_KEYS.ArrowUp || event.key === EVENT_KEYS.ArrowDown) &&
        Object.keys(this.shownFloat).length > 0
      ) {
        return
      }
      // Cursor outside editor area or over not editable elements.
      if (event.target.closest('[contenteditable=false]')) {
        return
      }

      // Ignore the event if it doesnt cause an edit in the editor (e.g control keys etc.)
      if (event.key in KEYS_TO_IGNORE) {
        return
      }

      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        // A composition may have started between the event and this tick.
        if (this.isComposed) {
          return
        }
        const cursor = selection.getCursorRange()
        if (!cursor.start || !cursor.end) {
          return
        }

        this.muya.dispatchSelectionChange(cursor)
        this.muya.dispatchSelectionFormats(cursor)
        if (!this.isComposed && event.type === 'click') {
          this.muya.dispatchChange()
        }
      })
    }

    eventCenter.attachDOMEvent(container, 'click', changeHandler)
    eventCenter.attachDOMEvent(container, 'keyup', changeHandler)
  }

  keydownBinding() {
    const { container, eventCenter, contentState } = this.muya
    const docHandler = (event) => {
      // Ignore IME-composition keys (incl. the keyCode-229 commit Enter) so they
      // don't trigger paragraph edits mid/just-after a composition. See `isImeKey`.
      if (isImeKey(event)) {
        return
      }
      switch (event.code) {
        case EVENT_KEYS.Enter:
          return contentState.docEnterHandler(event)
        case EVENT_KEYS.Space: {
          if (contentState.selectedImage) {
            const { token } = contentState.selectedImage
            const { src } = getImageInfo(token.src || token.attrs.src)
            if (src) {
              eventCenter.dispatch('preview-image', {
                data: src
              })
            }
          }
          break
        }
        case EVENT_KEYS.Backspace: {
          return contentState.docBackspaceHandler(event)
        }
        case EVENT_KEYS.Delete: {
          return contentState.docDeleteHandler(event)
        }
        case EVENT_KEYS.ArrowUp: // fallthrough
        case EVENT_KEYS.ArrowDown: // fallthrough
        case EVENT_KEYS.ArrowLeft: // fallthrough
        case EVENT_KEYS.ArrowRight: // fallthrough
          return contentState.docArrowHandler(event)
      }
    }

    const handler = (event) => {
      // Ignore IME-composition keys (incl. the keyCode-229 commit Enter); the
      // per-case `!this.isComposed` checks below are flag-based and race the
      // compositionend timing under WebKit, so this is the authoritative guard.
      if (isImeKey(event)) {
        return
      }
      if (event.metaKey || event.ctrlKey) {
        container.classList.add('ag-meta-or-ctrl')
      }

      if (
        Object.keys(this.shownFloat).length > 0 &&
        (event.key === EVENT_KEYS.Enter ||
          event.key === EVENT_KEYS.Escape ||
          event.key === EVENT_KEYS.Tab ||
          event.key === EVENT_KEYS.ArrowUp ||
          event.key === EVENT_KEYS.ArrowDown)
      ) {
        let needPreventDefault = false

        for (const tool in this.shownFloat) {
          if (
            tool === 'ag-format-picker' ||
            tool === 'ag-table-picker' ||
            tool === 'ag-quick-insert' ||
            tool === 'ag-emoji-picker' ||
            tool === 'ag-front-menu' ||
            tool === 'ag-list-picker' ||
            tool === 'ag-image-selector'
          ) {
            needPreventDefault = true
            break
          }
        }
        if (needPreventDefault) {
          event.preventDefault()
        }
        // event.stopPropagation()
        return
      }
      switch (event.key) {
        case EVENT_KEYS.Backspace:
          contentState.backspaceHandler(event)
          break
        case EVENT_KEYS.Delete:
          contentState.deleteHandler(event)
          break
        case EVENT_KEYS.Enter:
          if (!this.isComposed) {
            contentState.enterHandler(event)
            this.muya.dispatchChange()
          }
          break
        case EVENT_KEYS.ArrowUp: // fallthrough
        case EVENT_KEYS.ArrowDown: // fallthrough
        case EVENT_KEYS.ArrowLeft: // fallthrough
        case EVENT_KEYS.ArrowRight: // fallthrough
          if (!this.isComposed) {
            contentState.arrowHandler(event)
          }
          break
        case EVENT_KEYS.Tab:
          contentState.tabHandler(event)
          break
        default:
          break
      }
    }

    eventCenter.attachDOMEvent(container, 'keydown', handler)
    eventCenter.attachDOMEvent(document, 'keydown', docHandler)
  }

  // Handle Enter from `beforeinput`. Under WKWebView an IM-routed Enter can
  // carry keyCode 229 on keydown even after the composition has committed, so
  // the keydown handler (guarded by isImeKey) never processes it — and since
  // that guard returns without preventDefault, the engine would split the
  // paragraph natively, outside the model. The beforeinput inputType is
  // authoritative, so handle Enter here and block the native split. On
  // Chromium this handler never fires for Enter: enterHandler preventDefaults
  // the keydown first, which suppresses the beforeinput.
  beforeinputBinding() {
    const { container, eventCenter, contentState } = this.muya
    const handler = (event) => {
      if (this.isComposed || event.isComposing) {
        return
      }
      if (event.inputType !== 'insertParagraph' && event.inputType !== 'insertLineBreak') {
        return
      }
      event.preventDefault()
      // Mirror keydown: while a float tool is shown, Enter belongs to it.
      if (Object.keys(this.shownFloat).length > 0) {
        return
      }
      contentState.enterHandler({
        key: EVENT_KEYS.Enter,
        shiftKey: event.inputType === 'insertLineBreak',
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        preventDefault() {},
        stopPropagation() {}
      })
      this.muya.dispatchChange()
    }

    eventCenter.attachDOMEvent(container, 'beforeinput', handler)
  }

  inputBinding() {
    const { container, eventCenter, contentState } = this.muya
    const inputHandler = (event) => {
      // Composition-phase input must never touch the model — the DOM is
      // IME-owned until the composition commits. (Chromium tags these with
      // isComposing / insertCompositionText; the internal flag covers the rest.)
      if (this.isComposed || event.isComposing || event.inputType === 'insertCompositionText') {
        return
      }
      // WebKit fires the final composition input AFTER compositionend, with
      // isComposing already false — that event is the commit signal.
      if (event.inputType === 'insertFromComposition') {
        return this.commitComposition(event)
      }
      contentState.inputHandler(event)
      this.muya.dispatchChange()
      this.checkLanguageInput()
    }

    eventCenter.attachDOMEvent(container, 'input', inputHandler)
  }

  checkLanguageInput() {
    const { eventCenter, contentState } = this.muya
    const { lang, paragraph } = contentState.checkEditLanguage()
    if (lang) {
      eventCenter.dispatch('muya-code-picker', {
        reference: getParagraphReference(paragraph, paragraph.id),
        lang,
        cb: (item) => {
          contentState.selectLanguage(paragraph, item.name)
        }
      })
    } else {
      // hide code picker float box
      eventCenter.dispatch('muya-code-picker', { reference: null })
    }
  }

  keyupBinding() {
    const { container, eventCenter, contentState } = this.muya
    const handler = (event) => {
      // IME keyups (incl. the keyCode-229 commit Enter) must not run the
      // cursor sync below: around a commit the selection can be transiently
      // invalid, and writing that into contentState.cursor diverges it from
      // the block the composition committed to.
      if (this.isComposed || isImeKey(event)) {
        return
      }
      container.classList.remove('ag-meta-or-ctrl')
      // check if edit emoji
      const node = selection.getSelectionStart()
      const paragraph = findNearestParagraph(node)
      const emojiNode = checkEditEmoji(node)
      contentState.selectedImage = null
      if (
        paragraph &&
        emojiNode &&
        event.key !== EVENT_KEYS.Enter &&
        event.key !== EVENT_KEYS.ArrowDown &&
        event.key !== EVENT_KEYS.ArrowUp &&
        event.key !== EVENT_KEYS.Tab &&
        event.key !== EVENT_KEYS.Escape
      ) {
        const reference = getParagraphReference(emojiNode, paragraph.id)
        eventCenter.dispatch('muya-emoji-picker', {
          reference,
          emojiNode
        })
      }
      if (!emojiNode) {
        eventCenter.dispatch('muya-emoji-picker', {
          emojiNode
        })
      }

      const { anchor, focus, start, end } = selection.getCursorRange()
      if (!anchor || !focus) {
        return
      }
      if (!this.isComposed) {
        const { anchor: oldAnchor, focus: oldFocus } = contentState.cursor
        if (
          anchor.key !== oldAnchor.key ||
          anchor.offset !== oldAnchor.offset ||
          focus.key !== oldFocus.key ||
          focus.offset !== oldFocus.offset
        ) {
          const needRender =
            contentState.checkNeedRender(contentState.cursor) ||
            contentState.checkNeedRender({ start, end })
          contentState.cursor = { anchor, focus }
          if (needRender) {
            return contentState.partialRender()
          }
        }
      }

      const block = contentState.getBlock(anchor.key)
      if (
        anchor.key === focus.key &&
        anchor.offset !== focus.offset &&
        block.functionType !== 'codeContent' &&
        block.functionType !== 'languageInput'
      ) {
        const reference = contentState.getPositionReference()
        const { formats } = contentState.selectionFormats()
        eventCenter.dispatch('muya-format-picker', { reference, formats })
      } else {
        eventCenter.dispatch('muya-format-picker', { reference: null })
      }
    }

    eventCenter.attachDOMEvent(container, 'keyup', handler) // temp use input event
  }
}

export default Keyboard
