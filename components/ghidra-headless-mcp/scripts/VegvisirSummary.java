//Reports basic metadata for the current program after import/analysis.
//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Program;

public class VegvisirSummary extends GhidraScript {
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
                default:
                    if (c < 0x20) b.append(String.format("\\u%04x", (int)c));
                    else b.append(c);
            }
        }
        b.append('"');
        return b.toString();
    }
    @Override
    public void run() throws Exception {
        Program p = currentProgram;
        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"ok\": true,\n");
        out.append("  \"program\": ").append(q(p.getName())).append(",\n");
        out.append("  \"language\": ").append(q(p.getLanguageID().getIdAsString())).append(",\n");
        out.append("  \"compilerSpec\": ").append(q(p.getCompilerSpec().getCompilerSpecID().getIdAsString())).append(",\n");
        out.append("  \"imageBase\": ").append(q(p.getImageBase().toString())).append(",\n");
        out.append("  \"minAddress\": ").append(q(p.getMinAddress().toString())).append(",\n");
        out.append("  \"maxAddress\": ").append(q(p.getMaxAddress().toString())).append("\n");
        out.append("}");
        println("VEGVISIR_JSON:" + out.toString());
    }
}
