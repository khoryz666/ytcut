/** Small DOM helpers shared by every module. */

type Child = Node | string;

/** The element for a selector; throws if the page is missing it. */
export function $<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = document.querySelector<T>(selector);
  if (!found) throw new Error(`Missing element: ${selector}`);
  return found;
}

/** Create an element with properties and children. */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...children);
  return e;
}

export const button = (text: string, onclick: () => void, props: Partial<HTMLButtonElement> = {}) =>
  el('button', { type: 'button', textContent: text, onclick, ...props });

/** Flip a toggle button and return its new state. */
export function toggle(btn: HTMLElement): boolean {
  const on = btn.getAttribute('aria-pressed') !== 'true';
  btn.setAttribute('aria-pressed', String(on));
  return on;
}

/** Error message nodes, with a sign-in link when YouTube asks to prove you are not a bot. */
export function errorNodes(message: string): Child[] {
  if (!/sign in/i.test(message)) return [message];
  const link = el('a', { href: 'https://www.youtube.com/', target: '_blank', textContent: 'Sign in to YouTube' });
  return ['YouTube wants you to sign in. ', link, ', then try again.'];
}
