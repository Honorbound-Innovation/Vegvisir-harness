//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.data.StringDataInstance;

public class VegvisirListStrings extends GhidraScript {
    private static String q(String s) {
        if (s == null) return "null";
        StringBuilder b = new StringBuilder("\"");
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '\\': b.append("\\\\"); break;
                case '"': b.append("\\\""); break;
                case '\n': b.append("\\n"); break;
                case '\r': b.append("\\r"); break;
                case '\t': b.append("\\t"); break;
                default: if (c < 0x20) b.append(String.format("\\u%04x", (int)c)); else b.append(c);
            }
        }
        return b.append('"').toString();
    }
    @Override
    public void run() throws Exception {
        int limit = 200;
        int minLen = 4;
        String[] args = getScriptArgs();
        if (args.length > 0) limit = Integer.parseInt(args[0]);
        if (args.length > 1) minLen = Integer.parseInt(args[1]);
        StringBuilder out = new StringBuilder();
        out.append("{\n  \"ok\": true,\n  \"program\": ").append(q(currentProgram.getName())).append(",\n  \"strings\": [\n");
        DataIterator it = currentProgram.getListing().getDefinedData(true);
        int total = 0, emitted = 0;
        while (it.hasNext()) {
            Data d = it.next();
            StringDataInstance sdi = StringDataInstance.getStringDataInstance(d);
            if (sdi == null) continue;
            String value = sdi.getStringValue();
            if (value == null || value.length() < minLen) continue;
            total++;
            if (emitted >= limit) continue;
            if (emitted > 0) out.append(",\n");
            out.append("    {");
            out.append("\"address\": ").append(q(d.getAddress().toString())).append(", ");
            out.append("\"length\": ").append(value.length()).append(", ");
            out.append("\"value\": ").append(q(value));
            out.append("}");
            emitted++;
        }
        out.append("\n  ],\n  \"emitted\": ").append(emitted).append(",\n  \"total\": ").append(total).append("\n}");
        println("VEGVISIR_JSON:" + out.toString());
    }
}
