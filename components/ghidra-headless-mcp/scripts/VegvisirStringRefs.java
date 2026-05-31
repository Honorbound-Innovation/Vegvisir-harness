//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.data.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;

public class VegvisirStringRefs extends GhidraScript {
  private static String q(String s){ if(s==null)return"null"; StringBuilder b=new StringBuilder("\""); for(int i=0;i<s.length();i++){char c=s.charAt(i); switch(c){case '\\':b.append("\\\\");break;case '"':b.append("\\\"");break;case '\n':b.append("\\n");break;case '\r':b.append("\\r");break;case '\t':b.append("\\t");break;default: if(c<0x20)b.append(String.format("\\u%04x",(int)c)); else b.append(c);}} return b.append('"').toString(); }
  private String val(Data d){ try{Object v=d.getValue(); return v==null?null:v.toString();}catch(Exception e){return null;} }
  public void run() throws Exception { String[] a=getScriptArgs(); String query=a.length>0?a[0]:""; int limit=a.length>1?Integer.parseInt(a[1]):200; Listing listing=currentProgram.getListing(); ReferenceManager rm=currentProgram.getReferenceManager(); DataIterator it=listing.getDefinedData(true); StringBuilder out=new StringBuilder("{\n  \"ok\": true,\n  \"matches\": [\n"); int emitted=0,total=0; while(it.hasNext()){Data d=it.next(); if(!(d.getDataType() instanceof AbstractStringDataType)) continue; String s=val(d); if(s==null)continue; if(query.length()>0 && !s.toLowerCase().contains(query.toLowerCase())) continue; total++; if(emitted>=limit)continue; if(emitted++>0)out.append(",\n"); out.append("    {\"address\": ").append(q(d.getAddress().toString())).append(", \"value\": ").append(q(s)).append(", \"xrefs\": ["); ReferenceIterator refs=rm.getReferencesTo(d.getAddress()); int r=0; while(refs.hasNext()){Reference ref=refs.next(); if(r++>0)out.append(", "); out.append("{\"from\": ").append(q(ref.getFromAddress().toString())).append(", \"type\": ").append(q(ref.getReferenceType().toString())).append("}");} out.append("]}"); } out.append("\n  ],\n  \"emitted\": ").append(emitted).append(",\n  \"total\": ").append(total).append("\n}"); println("VEGVISIR_JSON:"+out); }
}
