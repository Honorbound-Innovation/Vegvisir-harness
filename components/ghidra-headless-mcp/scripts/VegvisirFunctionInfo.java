//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;

public class VegvisirFunctionInfo extends GhidraScript {
    private static String q(String s){ if(s==null)return"null"; StringBuilder b=new StringBuilder("\""); for(int i=0;i<s.length();i++){char c=s.charAt(i); switch(c){case '\\':b.append("\\\\");break;case '"':b.append("\\\"");break;case '\n':b.append("\\n");break;case '\r':b.append("\\r");break;case '\t':b.append("\\t");break;default: if(c<0x20)b.append(String.format("\\u%04x",(int)c)); else b.append(c);}} return b.append('"').toString(); }
    private Function find(String name, String addr) throws Exception { FunctionManager fm=currentProgram.getFunctionManager(); if(addr!=null && addr.length()>0){ Address a=currentProgram.getAddressFactory().getAddress(addr); if(a!=null){ Function f=fm.getFunctionAt(a); if(f==null) f=fm.getFunctionContaining(a); return f; }} if(name!=null && name.length()>0){ FunctionIterator it=fm.getFunctions(true); while(it.hasNext()){ Function f=it.next(); if(f.getName().equals(name) || f.getName(true).equals(name)) return f; }} return null; }
    public void run() throws Exception { String[] args=getScriptArgs(); String name=args.length>0?args[0]:""; String addr=args.length>1?args[1]:""; Function f=find(name,addr); if(f==null){ println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"function not found\"}"); return; }
        ReferenceManager rm=currentProgram.getReferenceManager(); ReferenceIterator to=rm.getReferencesTo(f.getEntryPoint()); int refsTo=0; while(to.hasNext()){to.next(); refsTo++;}
        StringBuilder out=new StringBuilder(); out.append("{\n  \"ok\": true,\n  \"program\": ").append(q(currentProgram.getName())).append(",\n  \"function\": {");
        out.append("\"name\": ").append(q(f.getName())).append(", "); out.append("\"fullName\": ").append(q(f.getName(true))).append(", "); out.append("\"entry\": ").append(q(f.getEntryPoint().toString())).append(", "); out.append("\"signature\": ").append(q(f.getSignature().getPrototypeString())).append(", "); out.append("\"bodyMin\": ").append(q(f.getBody().getMinAddress().toString())).append(", "); out.append("\"bodyMax\": ").append(q(f.getBody().getMaxAddress().toString())).append(", "); out.append("\"bodyNumAddresses\": ").append(f.getBody().getNumAddresses()).append(", "); out.append("\"external\": ").append(f.isExternal()).append(", \"thunk\": ").append(f.isThunk()).append(", \"refsToEntry\": ").append(refsTo).append(", "); out.append("\"comment\": ").append(q(f.getComment())); out.append("}\n}"); println("VEGVISIR_JSON:"+out.toString()); }
}
