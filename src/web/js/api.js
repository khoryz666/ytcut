/** Talking to the ytcut server. */

/** GET when data is omitted, POST form fields otherwise. Throws the server's message on error. */
export async function request(path, data) {
  const res = await fetch(path, data && { method: 'POST', body: new URLSearchParams(data) });
  if (!res.ok) throw new Error(await res.text());
  return res.headers.get('content-type').startsWith('application/json') ? res.json() : res.text();
}
