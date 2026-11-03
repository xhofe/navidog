import { View, div } from "gpui-kit";
import * as host from "navidog";
const {next_event,respond,respond_chunk,notify}=host;
export default class Service extends View {
  init(_props, cx) {
    cx.spawn(async () => {
      for (;;) {
        const event = await next_event();
        try {
          if (event.kind === "command") {
            if (event.command_id === "editor") host.insert_text(event.context, "-- Inserted by a plugin\n");
            else if (event.command_id === "grid") notify(event.context, "info", JSON.stringify(host.grid_selection(event.context)));
            else notify(event.context, "info", "Plugin command received");
          } else if (event.kind === "export") {
            respond(event.request_id, event.generation, { ok: true, data: JSON.stringify(event.arguments.input) });
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
