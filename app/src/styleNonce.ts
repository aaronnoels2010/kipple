// SPDX-License-Identifier: GPL-3.0-or-later
// Radix modals lock page scrolling with a <style> tag created at runtime (react-remove-scroll →
// react-style-singleton), which the release CSP `style-src 'self'` would block. Tauri gives every
// <style> in index.html a fresh nonce per load and allows it in the CSP (tauri-utils
// `inject_nonce_token`), so index.html carries an empty <style>, and its nonce is handed to
// get-nonce, where react-style-singleton reads it. The CSP itself stays strict.
import { setNonce } from "get-nonce";

export function adoptStyleNonce(doc: Document) {
  const nonce = doc.querySelector<HTMLStyleElement>("style[nonce]")?.nonce;
  if (nonce) setNonce(nonce);
}
