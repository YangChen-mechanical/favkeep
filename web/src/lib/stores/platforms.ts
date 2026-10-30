import api from '$lib/api';
import type { PlatformInfo } from '$lib/types';
import { derived, writable } from 'svelte/store';
export const platforms = writable<PlatformInfo[]>([]);
export const platformLabels = derived(platforms, (items) =>
	Object.fromEntries(items.map((p) => [p.id, p.name]))
);
export async function loadPlatforms() {
	platforms.set((await api.getPlatforms()).data);
}
