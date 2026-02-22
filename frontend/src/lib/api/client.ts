/** HTTP client for the ZeroClaw Cloud API. */

import type {
	DeviceSummary,
	DeviceInfo,
	ProvisionDeviceRequest,
	CommandEnvelope,
	CommandRecord,
	CommandSummary,
	SendCommandRequest,
	HealthResponse,
	TelemetryResponse
} from '$lib/types';

const BASE = '/api/v1';

class ApiClientError extends Error {
	constructor(
		public status: number,
		message: string
	) {
		super(message);
		this.name = 'ApiClientError';
	}
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
	const res = await fetch(path, {
		headers: { 'Content-Type': 'application/json', ...init?.headers },
		...init
	});

	if (!res.ok) {
		const body = await res.json().catch(() => ({ error: res.statusText }));
		throw new ApiClientError(res.status, body.error ?? res.statusText);
	}

	return res.json();
}

/** API client methods. */
export const api = {
	/** GET /health */
	health(): Promise<HealthResponse> {
		return request('/health');
	},

	/** GET /api/v1/devices */
	listDevices(): Promise<DeviceSummary[]> {
		return request(`${BASE}/devices`);
	},

	/** GET /api/v1/devices/:id */
	getDevice(id: string): Promise<DeviceInfo> {
		return request(`${BASE}/devices/${encodeURIComponent(id)}`);
	},

	/** POST /api/v1/devices */
	provisionDevice(req: ProvisionDeviceRequest): Promise<DeviceInfo> {
		return request(`${BASE}/devices`, {
			method: 'POST',
			body: JSON.stringify(req)
		});
	},

	/** GET /api/v1/devices/:id/telemetry */
	getTelemetry(id: string, source?: string, limit?: number): Promise<TelemetryResponse> {
		const params = new URLSearchParams();
		if (source) params.set('source', source);
		if (limit) params.set('limit', String(limit));
		const qs = params.toString();
		return request(`${BASE}/devices/${encodeURIComponent(id)}/telemetry${qs ? `?${qs}` : ''}`);
	},

	/** POST /api/v1/commands */
	sendCommand(req: SendCommandRequest): Promise<CommandEnvelope> {
		return request(`${BASE}/commands`, {
			method: 'POST',
			body: JSON.stringify(req)
		});
	},

	/** GET /api/v1/commands */
	listCommands(): Promise<CommandSummary[]> {
		return request(`${BASE}/commands`);
	},

	/** GET /api/v1/commands/:id */
	getCommand(id: string): Promise<CommandRecord> {
		return request(`${BASE}/commands/${encodeURIComponent(id)}`);
	}
};

export { ApiClientError };
