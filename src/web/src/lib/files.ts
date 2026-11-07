/** The library listing, shared by the sidebar and the download page. */
import { writable } from 'svelte/store';
import { api, errorMessage, type VideoFile } from './api';

export const files = writable<VideoFile[]>([]);
export const filesError = writable('');

export async function loadFiles() {
  try {
    files.set(await api.files());
    filesError.set('');
  } catch (e) {
    filesError.set(errorMessage(e));
  }
}
