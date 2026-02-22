export * from './device';
export * from './command';

export interface HealthResponse {
	status: string;
	version: string;
}

export interface TelemetryResponse {
	device_id: string;
	source: string | null;
	limit: number;
	readings: unknown[];
	message?: string;
}

export interface ApiError {
	error: string;
	status: number;
}

/** WebSocket event types matching server-side WsEvent. */
export type WsEvent =
	| {
			type: 'command_dispatched';
			command_id: string;
			device_id: string;
			command: string;
			initiated_by: string;
			created_at: string;
	  }
	| {
			type: 'command_response';
			command_id: string;
			device_id: string;
			status: string;
			inference_tier: string | null;
			response_text: string | null;
			latency_ms: number | null;
			responded_at: string;
	  }
	| {
			type: 'device_heartbeat';
			device_id: string;
			timestamp: string;
	  }
	| {
			type: 'device_status_changed';
			device_id: string;
			old_status: string;
			new_status: string;
			changed_at: string;
	  };
