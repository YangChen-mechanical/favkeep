export function formatTime(seconds: number): string {
	const total = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
	const minutes = Math.floor(total / 60);
	const rest = String(total % 60).padStart(2, '0');
	return minutes >= 60
		? `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}:${rest}`
		: `${minutes}:${rest}`;
}

export function returnUrl(value: string | null): string {
	return value && /^\/watch(?:\?[^#\\]*)?$/.test(value) ? value : '/watch';
}

export function playbackUrl(id: number, part: number | undefined, search: string): string {
	const params = new URLSearchParams();
	if (part) params.set('part', String(part));
	params.set('return', `/watch${search}`);
	return `/watch/${id}?${params}`;
}
