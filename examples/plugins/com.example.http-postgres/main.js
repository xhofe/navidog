import { View, div } from "gpui-kit";
import { next_event, respond, respond_chunk } from "navidog";
export default class Service extends View {
  init(_props, cx) {
    cx.spawn(async () => {
      for (;;) {
        const event = await next_event();
        if (event.kind !== "driver.request" && event.kind !== "cancel") continue;
        cx.spawn(async () => {
          try {
            const response = await fetch("http://127.0.0.1:18123/request", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(event) });
            const result = await response.json();
            if (event.kind === "cancel") return;
            if (!result.ok) throw new Error(result.error);
            for (let set = 0; set < (result.results || []).length; set++) {
              const value = result.results[set];
              if (!value.rows.length) respond_chunk(event.request_id, event.generation, { ...value, result_set: set, sequence: 0, end: true });
              for (let offset = 0, sequence = 0; offset < value.rows.length; offset += 1000, sequence++) {
                respond_chunk(event.request_id, event.generation, { result_set: set, sequence, ...(sequence === 0 ? { columns: value.columns } : {}), rows: value.rows.slice(offset, offset + 1000), end: offset + 1000 >= value.rows.length, ...(offset + 1000 >= value.rows.length ? { affected_rows: value.affected_rows, notices: value.notices } : {}) });
              }
            }
            respond(event.request_id, event.generation, { ok: true, data: result.data });
          } catch (error) {
            if (event.request_id && event.kind !== "cancel") respond(event.request_id, event.generation, { ok: false, error: String(error) });
          }
        });
      }
    });
  }
  render() { return div(); }
}
