//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.program.model.symbol.*;

public class VegvisirListImports extends GhidraScript {
    private static String q(String s){ if(s==null)return"null"; StringBuilder b=new StringBuilder("\""); for(int i=0;i<s.length();i++){char c=s.charAt(i); switch(c){case '\\':b.append("\\\\");break;case '"':b.append("\\\"");break;case '\n':b.append("\\n");break;case '\r':b.append("\\r");break;case '\t':b.append("\\t");break;default: if(c<0x20)b.append(String.format("\\u%04x",(int)c)); else b.append(c);}} return b.append('"').toString(); }
    public void run() throws Exception {
        int limit=200; String[] args=getScriptArgs(); if(args.length>0)limit=Integer.parseInt(args[0]);
        SymbolTable st=currentProgram.getSymbolTable(); SymbolIterator it=st.getExternalSymbols();
        StringBuilder out=new StringBuilder(); out.append("{\n  \"ok\": true,\n  \"program\": ").append(q(currentProgram.getName())).append(",\n  \"imports\": [\n");
        int total=0, emitted=0; while(it.hasNext()){ Symbol s=it.next(); total++; if(emitted>=limit)continue; if(emitted>0)out.append(",\n");
            out.append("    {"); out.append("\"name\": ").append(q(s.getName(true))).append(", "); out.append("\"address\": ").append(q(s.getAddress().toString())).append(", "); out.append("\"type\": ").append(q(s.getSymbolType().toString())).append(", "); out.append("\"namespace\": ").append(q(s.getParentNamespace()==null?null:s.getParentNamespace().getName(true))); out.append("}"); emitted++; }
        out.append("\n  ],\n  \"emitted\": ").append(emitted).append(",\n  \"total\": ").append(total).append("\n}"); println("VEGVISIR_JSON:"+out.toString());
    }
}
