//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;

public class VegvisirListFunctions extends GhidraScript {
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
        String[] args = getScriptArgs();
        if (args.length > 0) limit = Integer.parseInt(args[0]);
        StringBuilder out = new StringBuilder();
        out.append("{\n  \"ok\": true,\n  \"program\": ").append(q(currentProgram.getName())).append(",\n  \"functions\": [\n");
        FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
        int count = 0;
        int emitted = 0;
        while (it.hasNext()) {
            Function f = it.next();
            count++;
            if (emitted >= limit) continue;
            if (emitted > 0) out.append(",\n");
            out.append("    {");
            out.append("\"name\": ").append(q(f.getName())).append(", ");
            out.append("\"entry\": ").append(q(f.getEntryPoint().toString())).append(", ");
            out.append("\"signature\": ").append(q(f.getSignature().getPrototypeString())).append(", ");
            out.append("\"thunk\": ").append(f.isThunk()).append(", ");
            out.append("\"external\": ").append(f.isExternal());
            out.append("}");
            emitted++;
        }
        out.append("\n  ],\n  \"emitted\": ").append(emitted).append(",\n  \"total\": ").append(count).append("\n}");
        println("VEGVISIR_JSON:" + out.toString());
    }
}
