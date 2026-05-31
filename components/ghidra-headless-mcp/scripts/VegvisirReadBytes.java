//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.Memory;

public class VegvisirReadBytes extends GhidraScript {
    private static String q(String s){ if(s==null)return"null"; StringBuilder b=new StringBuilder("\""); for(int i=0;i<s.length();i++){char c=s.charAt(i); switch(c){case '\\':b.append("\\\\");break;case '"':b.append("\\\"");break;case '\n':b.append("\\n");break;case '\r':b.append("\\r");break;case '\t':b.append("\\t");break;default: if(c<0x20)b.append(String.format("\\u%04x",(int)c)); else b.append(c);}} return b.append('"').toString(); }
    public void run() throws Exception { String[] args=getScriptArgs(); String addrS=args.length>0?args[0]:""; int len=args.length>1?Integer.parseInt(args[1]):64; if(len<0)len=0; if(len>4096)len=4096; Address a=currentProgram.getAddressFactory().getAddress(addrS); if(a==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"invalid address\"}");return;} byte[] buf=new byte[len]; Memory mem=currentProgram.getMemory(); int read=mem.getBytes(a,buf); StringBuilder hex=new StringBuilder(); StringBuilder ascii=new StringBuilder(); for(int i=0;i<read;i++){ if(i>0)hex.append(' '); int v=buf[i]&0xff; hex.append(String.format("%02x",v)); ascii.append(v>=32 && v<127 ? (char)v : '.'); } StringBuilder out=new StringBuilder(); out.append("{\n  \"ok\": true,\n  \"address\": ").append(q(a.toString())).append(",\n  \"requested\": ").append(len).append(",\n  \"read\": ").append(read).append(",\n  \"hex\": ").append(q(hex.toString())).append(",\n  \"ascii\": ").append(q(ascii.toString())).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); }
}
