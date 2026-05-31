//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.program.model.mem.*;

public class VegvisirListSegments extends GhidraScript {
    private static String q(String s){ if(s==null)return"null"; StringBuilder b=new StringBuilder("\""); for(int i=0;i<s.length();i++){char c=s.charAt(i); switch(c){case '\\':b.append("\\\\");break;case '"':b.append("\\\"");break;case '\n':b.append("\\n");break;case '\r':b.append("\\r");break;case '\t':b.append("\\t");break;default: if(c<0x20)b.append(String.format("\\u%04x",(int)c)); else b.append(c);}} return b.append('"').toString(); }
    public void run() throws Exception { Memory mem=currentProgram.getMemory(); StringBuilder out=new StringBuilder(); out.append("{\n  \"ok\": true,\n  \"program\": ").append(q(currentProgram.getName())).append(",\n  \"segments\": [\n"); int emitted=0; for(MemoryBlock b: mem.getBlocks()){ if(emitted>0)out.append(",\n"); out.append("    {"); out.append("\"name\": ").append(q(b.getName())).append(", "); out.append("\"start\": ").append(q(b.getStart().toString())).append(", "); out.append("\"end\": ").append(q(b.getEnd().toString())).append(", "); out.append("\"size\": ").append(b.getSize()).append(", "); out.append("\"read\": ").append(b.isRead()).append(", \"write\": ").append(b.isWrite()).append(", \"execute\": ").append(b.isExecute()).append(", \"initialized\": ").append(b.isInitialized()); out.append("}"); emitted++; } out.append("\n  ],\n  \"emitted\": ").append(emitted).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); }
}
