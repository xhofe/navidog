declare module "navidog" {
  export type Cell = string | { base64: string } | null;
  export interface Column { name: string; type_name: string; origin?: { schema: string; table: string; column: string } }
  export interface ResultSet { columns: Column[]; rows: Cell[][]; affected_rows?: string | null; notices?: string[] }
  export interface ResultChunk { columns?: Column[]; rows: Cell[][]; affected_rows?: string | null; notices?: string[]; result_set: number; sequence: number; end: boolean }
  export interface Event { kind: "command" | "driver.request" | "import" | "export" | "export.chunk" | "cancel"; generation: number; request_id?: string; command_id?: string; context?: string; op?: string; arguments?: Record<string, unknown> }
  export type Reply = { ok: true; data?: unknown } | { ok: false; error: string; code?: string };
  export function api_version(): number;
  export function locale(): string;
  export function next_event(): Promise<Event>;
  export function respond(id: string, generation: number, reply: Reply): void;
  export function respond_chunk(id: string, generation: number, chunk: ResultChunk): void;
  export function notify(context: string, level: "info" | "success" | "warning" | "error", message: string): void;
  export function list_connections(): { id: string; name: string; driver: string; host: string; color: unknown }[];
  export function grid_selection(context: string): ResultSet;
  export function insert_text(context: string, text: string): void;
  export function execute(context: string, connection: string, database: string | null, sql: string): Promise<ResultSet[]>;
  export function import_rows(context: string, format: string, input: string): Promise<ResultSet[]>;
  export function export_rows(context: string, format: string, result: ResultSet): Promise<string>;
}
