export * from './device';
export * from './command';

export interface HealthResponse {
	status: string;
	version: string;
}

export type TelemetrySource = 'obd2' | 'system' | 'canbus';

export interface TelemetryReading {
	time: string;
	metric_name: string;
	value_numeric: number | null;
	value_text: string | null;
	value_json: unknown | null;
	unit: string | null;
	source: TelemetrySource;
}

export interface TelemetryResponse {
	device_id: string;
	source: string | null;
	limit: number;
	readings: TelemetryReading[];
	message?: string;
}

export interface ApiError {
	error: string;
	status: number;
}

export interface ShadowSummary {
	shadow_name: string;
	version: number;
	last_updated: string;
}

export interface ShadowResponse {
	device_id: string;
	shadow_name: string;
	reported: unknown;
	desired: unknown;
	delta: unknown;
	version: number;
	last_updated: string;
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
			response_data: unknown | null;
			error: string | null;
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
	  }
	| {
			type: 'device_provisioned';
			device_id: string;
			fleet_id: string;
			hardware_type: string;
			provisioned_at: string;
	  }
	| {
			type: 'telemetry_ingested';
			device_id: string;
			count: number;
			source: string;
			timestamp: string;
	  }
	| {
			type: 'shadow_updated';
			device_id: string;
			shadow_name: string;
			version: number;
			timestamp: string;
	  };
