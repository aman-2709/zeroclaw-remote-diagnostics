/** Shared formatting utilities. */

/** Relative time string from an ISO timestamp. */
export function timeAgo(iso: string): string {
	const diff = Date.now() - new Date(iso).getTime();
	const secs = Math.floor(diff / 1000);
	if (secs < 60) return 'just now';
	const mins = Math.floor(secs / 60);
	if (mins < 60) return `${mins}m ago`;
	const hours = Math.floor(mins / 60);
	if (hours < 24) return `${hours}h ago`;
	return `${Math.floor(hours / 24)}d ago`;
}

/** Compact date/time display. */
export function shortDateTime(iso: string): string {
	const d = new Date(iso);
	const month = String(d.getMonth() + 1).padStart(2, '0');
	const day = String(d.getDate()).padStart(2, '0');
	const hours = String(d.getHours()).padStart(2, '0');
	const mins = String(d.getMinutes()).padStart(2, '0');
	return `${month}/${day} ${hours}:${mins}`;
}

/** Format seconds into human-readable uptime. */
export function formatUptime(secs: number): string {
	const days = Math.floor(secs / 86400);
	const hours = Math.floor((secs % 86400) / 3600);
	const mins = Math.floor((secs % 3600) / 60);
	if (days > 0) return `${days}d ${hours}h ${mins}m`;
	if (hours > 0) return `${hours}h ${mins}m`;
	return `${mins}m`;
}
