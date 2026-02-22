/** Mirrors zc-protocol command types. */

export type CommandStatus = 'pending' | 'sent' | 'received' | 'executing' | 'completed' | 'failed';

export type InferenceTier = 'local' | 'cloud';

export interface CommandEnvelope {
	id: string;
	fleet_id: string;
	device_id: string;
	natural_language: string;
	initiated_by: string;
	timestamp: string;
}

export interface CommandResponse {
	command_id: string;
	device_id: string;
	status: CommandStatus;
	inference_tier: InferenceTier;
	result: Record<string, unknown> | null;
	error: string | null;
	latency_ms: number;
	timestamp: string;
}

export interface CommandRecord {
	command: CommandEnvelope;
	response: CommandResponse | null;
	created_at: string;
}

export interface CommandSummary {
	id: string;
	device_id: string;
	command: string;
	status: CommandStatus | null;
	created_at: string;
}

export interface SendCommandRequest {
	device_id: string;
	fleet_id: string;
	command: string;
	initiated_by: string;
}
