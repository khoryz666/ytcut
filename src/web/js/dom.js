/** Small DOM helpers shared by every module. */

export const $ = (selector) => document.querySelector(selector);

/** Create an element with properties and children. */
export function el(tag, props, ...children) {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...children);
  return e;
}

export const button = (textContent, onclick, props) =>
  el('button', { type: 'button', textContent, onclick, ...props });

/** Flip a toggle button and return its new state. */
export function toggle(btn) {
  const on = btn.getAttribute('aria-pressed') !== 'true';
  btn.setAttribute('aria-pressed', on);
  return on;
}

/** Error message nodes, with a sign-in link when YouTube asks to prove you are not a bot. */
export const errorNodes = (msg) => /sign in/i.test(msg)
  ? ['YouTube wants you to sign in. ',
     el('a', { href: 'https://www.youtube.com/', target: '_blank', textContent: 'Sign in to YouTube' }),
     ', then try again.']
  : [msg];
