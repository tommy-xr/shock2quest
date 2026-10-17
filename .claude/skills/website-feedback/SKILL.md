---
name: website-feedback
description: Apply a feedback export from the landing page review overlay (website/?feedback=1 or its bookmarklet; a feedback-*.json with format shock2quest-website-feedback/1, by default the newest in website/feedback/). Maps each marked note to source lines, makes the change, and render-verifies it at the reviewer's viewport. Use when given such a file or asked to act on website feedback.
---

# Website feedback

The reviewer opened `website/?feedback=1` (or used the bookmarklet on the live
site), marked parts of the page with boxes or circles, noted each, and exported
JSON (shape in `website/README.md`). Exports live in `website/feedback/`; use
the newest there unless given a path, and commit or delete it in the fix PR.

1. **Locate.** `node .claude/skills/website-feedback/scripts/locate.mjs [feedback.json]`
   (default: newest `website/feedback/*.json` by its `created` time)
   prints each note with its selector, element text, shape, rect and viewport, plus
   candidate source lines: the section's `id=`, the element's `data-*`
   attribute, then its text with markup stripped. Copy assembled at runtime
   (control-mode chips, disclosure text, feeds) is found in
   `website/shared/controls.js` by its text.
2. **Read the mark, not just the element.** `selector`/`text` name the element
   that best matches the mark's rect (a circle is the ellipse inscribed in it);
   `rect` (document CSS px) and `viewport` say what
   the reviewer actually framed. A note may be about spacing around the
   element, or about a narrow viewport only (the page reflows at 900px).
3. **Edit** the narrowest thing that answers the note. If a note is ambiguous
   or conflicts with another, ask rather than guess. `overall` is page-wide
   direction; apply it only where it is concrete.
4. **Verify** each change rendered at the annotation's `viewport`. Serve the repo
   root (`python3 -m http.server 4180`), open `/website/`, scroll to
   `rect.y`, screenshot before and after. Run `node --test website/test/*.test.js`.
5. **Report** per note: done (what changed, file:line) or skipped (why).

A selector that no longer resolves means the page changed since the export;
fall back to the text and the box position. The export does not record which
controls tab or equipment-tour step was showing, so for boxes in those widgets
trust `text` over `selector`; for the `#clip` video (no text) ask which mode.
