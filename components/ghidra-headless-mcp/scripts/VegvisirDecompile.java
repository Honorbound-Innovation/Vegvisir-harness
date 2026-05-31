//@category Vegvisir
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;

public class VegvisirDecompile extends GhidraScript {
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
    private Function findByName(String name) {
        FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
        while (it.hasNext()) {
            Function f = it.next();
            if (f.getName().equals(name)) return f;
        }
        return null;
    }
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String name = args.length > 0 ? args[0] : "";
        String addressText = args.length > 1 ? args[1] : "";
        int timeout = args.length > 2 ? Integer.parseInt(args[2]) : 30;
        int maxChars = args.length > 3 ? Integer.parseInt(args[3]) : 20000;
        Function f = null;
        if (addressText != null && !addressText.isBlank()) {
            Address a = currentProgram.getAddressFactory().getAddress(addressText);
            if (a != null) f = currentProgram.getFunctionManager().getFunctionAt(a);
            if (f == null && a != null) f = currentProgram.getFunctionManager().getFunctionContaining(a);
        }
        if (f == null && name != null && !name.isBlank()) f = findByName(name);
        if (f == null) {
            println("VEGVISIR_JSON:{\"ok\": false, \"error\": \"function not found\"}");
            return;
        }
        DecompInterface ifc = new DecompInterface();
        DecompileOptions opts = new DecompileOptions();
        opts.grabFromProgram(currentProgram);
        ifc.setOptions(opts);
        ifc.openProgram(currentProgram);
        DecompileResults res = ifc.decompileFunction(f, timeout, monitor);
        String code = res.getDecompiledFunction() == null ? "" : res.getDecompiledFunction().getC();
        boolean truncated = false;
        if (code.length() > maxChars) {
            code = code.substring(0, maxChars);
            truncated = true;
        }
        StringBuilder out = new StringBuilder();
        out.append("{\n  \"ok\": ").append(res.decompileCompleted()).append(",\n");
        out.append("  \"program\": ").append(q(currentProgram.getName())).append(",\n");
        out.append("  \"function\": {\"name\": ").append(q(f.getName())).append(", \"entry\": ").append(q(f.getEntryPoint().toString())).append("},\n");
        out.append("  \"decompileCompleted\": ").append(res.decompileCompleted()).append(",\n");
        out.append("  \"errorMessage\": ").append(q(res.getErrorMessage())).append(",\n");
        out.append("  \"truncated\": ").append(truncated).append(",\n");
        out.append("  \"c\": ").append(q(code)).append("\n}");
        println("VEGVISIR_JSON:" + out.toString());
        ifc.dispose();
    }
}
