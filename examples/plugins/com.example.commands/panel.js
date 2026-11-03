import { View, div } from "gpui-kit";
import { Dialog, Text } from "gpui-component";
export default class PluginHelp extends View {
  render() {
    return div().child("Plugin panel remains in the Dock when disabled.")
      .child(new Dialog("plugin-about", "About plugin…", () => {})
        .title("Plugin panel")
        .content(new Text("Native component overlays use the host window Root.")));
  }
}
