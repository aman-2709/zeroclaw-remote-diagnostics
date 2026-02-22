/** Reactive WebSocket client for real-time events. */

import type { WsEvent } from '$lib/types';

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected';

export type WsEventHandler = (event: WsEvent) => void;

const RECONNECT_DELAY_MS = 3000;
const MAX_RECONNECT_DELAY_MS = 30000;

class WebSocketStore {
	status = $state<ConnectionStatus>('disconnected');
	lastEvent = $state<WsEvent | null>(null);

	private ws: WebSocket | null = null;
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private reconnectDelay = RECONNECT_DELAY_MS;
	private handlers: Set<WsEventHandler> = new Set();
	private shouldReconnect = false;

	/** Connect to the WebSocket endpoint. */
	connect() {
		if (this.ws?.readyState === WebSocket.OPEN || this.ws?.readyState === WebSocket.CONNECTING) {
			return;
		}

		this.shouldReconnect = true;
		this.status = 'connecting';

		const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
		const url = `${protocol}//${window.location.host}/api/v1/ws`;

		this.ws = new WebSocket(url);

		this.ws.onopen = () => {
			this.status = 'connected';
			this.reconnectDelay = RECONNECT_DELAY_MS;
		};

		this.ws.onmessage = (msg) => {
			try {
				const event: WsEvent = JSON.parse(msg.data);
				this.lastEvent = event;
				for (const handler of this.handlers) {
					handler(event);
				}
			} catch {
				// Ignore malformed messages
			}
		};

		this.ws.onclose = () => {
			this.status = 'disconnected';
			this.ws = null;
			if (this.shouldReconnect) {
				this.scheduleReconnect();
			}
		};

		this.ws.onerror = () => {
			this.ws?.close();
		};
	}

	/** Disconnect and stop reconnecting. */
	disconnect() {
		this.shouldReconnect = false;
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
		this.ws?.close();
		this.ws = null;
		this.status = 'disconnected';
	}

	/** Register an event handler. Returns an unsubscribe function. */
	onEvent(handler: WsEventHandler): () => void {
		this.handlers.add(handler);
		return () => this.handlers.delete(handler);
	}

	private scheduleReconnect() {
		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = null;
			this.connect();
		}, this.reconnectDelay);
		// Exponential backoff with cap
		this.reconnectDelay = Math.min(this.reconnectDelay * 2, MAX_RECONNECT_DELAY_MS);
	}
}

/** Singleton WebSocket store. */
export const wsStore = new WebSocketStore();
