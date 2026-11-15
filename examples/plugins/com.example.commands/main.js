import { View, div } from "gpui-kit";
import * as host from "navidog";
const {next_event,respond,respond_chunk,notify}=host;
export default class Service extends View {
  init(_props, cx) {
    const exports = new Map();
    cx.spawn(async () => {
      for (;;) {
        const event = await next_event();
        try {
          if (event.kind === "command") {
            if (event.command_id === "editor") host.insert_text(event.context, "-- Inserted by a plugin\n");
            else if (event.command_id === "grid") notify(event.context, "info", JSON.stringify(host.grid_selection(event.context)));
            else notify(event.context, "info", "Plugin command received");
          } else if (event.kind === "export") {
            exports.set(event.request_id, { rows: [], sequence: 0 });
          } else if (event.kind === "export.chunk") {
            const state=exports.get(event.request_id);
            if (!state || event.chunk.sequence !== state.sequence++) throw new Error("Invalid input batch");
            state.rows.push(...event.chunk.rows);
            if (event.chunk.end) {respond(event.request_id,event.generation,{ok:true,data:JSON.stringify(state.rows)});exports.delete(event.request_id);}
          } else if (event.kind === "cancel") {
            exports.delete(event.request_id);
          } else if (event.kind === "import") {
            const rows = JSON.parse(event.arguments.input);
            respond_chunk(event.request_id, event.generation, { result_set: 0, sequence: 0, columns: [{ name: "value", type_name: "text" }], rows: rows.map(v => [String(v)]), end: true });
            respond(event.request_id, event.generation, { ok: true });
          }
        } catch (error) {
          if (event.request_id) respond(event.request_id, event.generation, { ok: false, error: String(error) });
        }
      }
    });
  }
  render() { return div(); }
}
