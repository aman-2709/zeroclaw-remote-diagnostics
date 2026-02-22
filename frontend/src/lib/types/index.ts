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
