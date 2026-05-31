//@category Vegvisir
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.function.ApplyFunctionSignatureCmd;
import ghidra.app.cmd.function.FunctionRenameOption;
import ghidra.app.util.parser.FunctionSignatureParser;
import ghidra.app.decompiler.*;
import ghidra.program.model.pcode.*;
import ghidra.util.data.DataTypeParser;
import ghidra.util.data.DataTypeParser.AllowedDataTypes;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.data.*;
import ghidra.program.model.mem.*;
import java.nio.file.*;
import java.time.*;
import java.util.*;

public class VegvisirMutate extends GhidraScript {
    private static String q(String s){ if(s==null)return"null"; StringBuilder b=new StringBuilder("\""); for(int i=0;i<s.length();i++){char c=s.charAt(i); switch(c){case '\\':b.append("\\\\");break;case '"':b.append("\\\"");break;case '\n':b.append("\\n");break;case '\r':b.append("\\r");break;case '\t':b.append("\\t");break;default: if(c<0x20)b.append(String.format("\\u%04x",(int)c)); else b.append(c);}} return b.append('"').toString(); }
    private Function findFunction(String addrS) throws Exception { Address a=currentProgram.getAddressFactory().getAddress(addrS); if(a==null)return null; Function f=currentProgram.getFunctionManager().getFunctionAt(a); if(f==null)f=currentProgram.getFunctionManager().getFunctionContaining(a); return f; }

    private DataType parseDataType(String text) throws Exception {
        DataTypeParser parser = new DataTypeParser(currentProgram.getDataTypeManager(), currentProgram.getDataTypeManager(), null, AllowedDataTypes.ALL);
        DataType dt = parser.parse(text);
        if (dt.getDataTypeManager() != currentProgram.getDataTypeManager()) {
            dt = currentProgram.getDataTypeManager().resolve(dt, DataTypeConflictHandler.DEFAULT_HANDLER);
        }
        return dt;
    }
    private HighFunction decompileHigh(Function f, int timeoutSeconds) throws Exception {
        DecompInterface ifc = new DecompInterface();
        ifc.toggleCCode(true);
        ifc.toggleSyntaxTree(true);
        ifc.setSimplificationStyle("decompile");
        if (!ifc.openProgram(currentProgram)) throw new Exception("failed to open decompiler");
        DecompileResults res = ifc.decompileFunction(f, timeoutSeconds, monitor);
        if (!res.decompileCompleted()) throw new Exception("decompile failed: " + res.getErrorMessage());
        HighFunction hf = res.getHighFunction();
        if (hf == null) throw new Exception("decompiler did not return high function");
        return hf;
    }
    private HighSymbol findHighSymbol(Function f, String name) throws Exception {
        HighFunction hf = decompileHigh(f, 30);
        Iterator<HighSymbol> it = hf.getLocalSymbolMap().getSymbols();
        while (it.hasNext()) {
            HighSymbol sym = it.next();
            if (sym != null && name.equals(sym.getName())) return sym;
        }
        throw new Exception("variable not found in decompiler symbols: " + name);
    }
    private String dataTypeSummary(DataType dt) {
        if (dt == null) return null;
        return dt.getDisplayName() + " (" + dt.getLength() + " bytes)";
    }
    public void run() throws Exception { String[] args=getScriptArgs(); if(args.length<1){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"action required\"}");return;} String action=args[0]; boolean dry=false; for(String a: args){ if(a.equals("--dry-run")) dry=true; }
        StringBuilder out=new StringBuilder(); out.append("{\n  \"ok\": true,\n  \"dryRun\": ").append(dry).append(",\n  \"action\": ").append(q(action)).append(",\n");
        if(action.equals("rename-function")){ if(args.length<3){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"rename-function address newName required\"}");return;} Function f=findFunction(args[1]); if(f==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"function not found\"}");return;} String old=f.getName(); String newName=args[2]; if(!dry){ f.setName(newName, SourceType.USER_DEFINED); } out.append("  \"address\": ").append(q(f.getEntryPoint().toString())).append(",\n  \"oldName\": ").append(q(old)).append(",\n  \"newName\": ").append(q(newName)).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }
        if(action.equals("set-comment")){ if(args.length<4){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"set-comment address type text required\"}");return;} Address a=currentProgram.getAddressFactory().getAddress(args[1]); if(a==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"invalid address\"}");return;} String type=args[2]; String text=args[3]; Listing listing=currentProgram.getListing(); CodeUnit cu=listing.getCodeUnitAt(a); if(cu==null)cu=listing.getCodeUnitContaining(a); if(cu==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"code unit not found\"}");return;} int ct=CodeUnit.EOL_COMMENT; if(type.equals("pre"))ct=CodeUnit.PRE_COMMENT; else if(type.equals("post"))ct=CodeUnit.POST_COMMENT; else if(type.equals("plate"))ct=CodeUnit.PLATE_COMMENT; else if(type.equals("repeatable"))ct=CodeUnit.REPEATABLE_COMMENT; String old=cu.getComment(ct); if(!dry){ cu.setComment(ct,text); } out.append("  \"address\": ").append(q(cu.getAddress().toString())).append(",\n  \"commentType\": ").append(q(type)).append(",\n  \"oldComment\": ").append(q(old)).append(",\n  \"newComment\": ").append(q(text)).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }

        if(action.equals("set-function-comment")){ if(args.length<3){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"set-function-comment address text required\"}");return;} Function f=findFunction(args[1]); if(f==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"function not found\"}");return;} String old=f.getComment(); String text=args[2]; if(!dry){ f.setComment(text); } out.append("  \"address\": ").append(q(f.getEntryPoint().toString())).append(",\n  \"oldComment\": ").append(q(old)).append(",\n  \"newComment\": ").append(q(text)).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }
        if(action.equals("set-function-signature")){ if(args.length<3){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"set-function-signature address signature required\"}");return;} Function f=findFunction(args[1]); if(f==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"function not found\"}");return;} String old=f.getSignature().getPrototypeString(); String sig=args[2]; FunctionSignatureParser parser=new FunctionSignatureParser(currentProgram.getDataTypeManager(), null); FunctionDefinitionDataType parsed=parser.parse(f.getSignature(), sig); String parsedSig=parsed.getPrototypeString(); if(!dry){ ApplyFunctionSignatureCmd cmd=new ApplyFunctionSignatureCmd(f.getEntryPoint(), parsed, SourceType.USER_DEFINED, true, false, DataTypeConflictHandler.DEFAULT_HANDLER, FunctionRenameOption.RENAME); if(!cmd.applyTo(currentProgram, monitor)){ println("VEGVISIR_JSON:{\"ok\":false,\"error\":"+q(cmd.getStatusMsg())+",\"oldSignature\":"+q(old)+",\"requestedSignature\":"+q(sig)+"}"); return; } } out.append("  \"address\": ").append(q(f.getEntryPoint().toString())).append(",\n  \"oldSignature\": ").append(q(old)).append(",\n  \"requestedSignature\": ").append(q(sig)).append(",\n  \"parsedSignature\": ").append(q(parsedSig)).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }
        if(action.equals("rename-variable")){ if(args.length<4){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"rename-variable functionAddress oldName newName required\"}");return;} Function f=findFunction(args[1]); if(f==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"function not found\"}");return;} HighSymbol sym=findHighSymbol(f,args[2]); String oldName=sym.getName(); String newName=args[3]; if(!dry){ HighFunctionDBUtil.updateDBVariable(sym,newName,null,SourceType.USER_DEFINED); } out.append("  \"functionAddress\": ").append(q(f.getEntryPoint().toString())).append(",\n  \"oldName\": ").append(q(oldName)).append(",\n  \"newName\": ").append(q(newName)).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }
        if(action.equals("set-variable-type")){ if(args.length<4){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"set-variable-type functionAddress variable type required\"}");return;} Function f=findFunction(args[1]); if(f==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"function not found\"}");return;} HighSymbol sym=findHighSymbol(f,args[2]); DataType dt=parseDataType(args[3]); String oldType=dataTypeSummary(sym.getDataType()); if(!dry){ HighFunctionDBUtil.updateDBVariable(sym,null,dt,SourceType.USER_DEFINED); } out.append("  \"functionAddress\": ").append(q(f.getEntryPoint().toString())).append(",\n  \"variable\": ").append(q(sym.getName())).append(",\n  \"oldType\": ").append(q(oldType)).append(",\n  \"newType\": ").append(q(dataTypeSummary(dt))).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }
        if(action.equals("apply-data-type")){ if(args.length<4){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"apply-data-type address type length required\"}");return;} Address a=currentProgram.getAddressFactory().getAddress(args[1]); if(a==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"invalid address\"}");return;} DataType dt=parseDataType(args[2]); int len=Integer.parseInt(args[3]); if(!dry){ DataUtilities.createData(currentProgram,a,dt,len,DataUtilities.ClearDataMode.CLEAR_ALL_CONFLICT_DATA); } out.append("  \"address\": ").append(q(a.toString())).append(",\n  \"dataType\": ").append(q(dataTypeSummary(dt))).append(",\n  \"length\": ").append(len).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }
        if(action.equals("create-struct")){ if(args.length<3){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"create-struct name size required\"}");return;} String name=args[1]; int size=Integer.parseInt(args[2]); StructureDataType st=new StructureDataType(name,0); st.add(new ArrayDataType(ByteDataType.dataType,size,1),"bytes",null); DataType resolved=st; if(!dry){ resolved=currentProgram.getDataTypeManager().resolve(st,DataTypeConflictHandler.DEFAULT_HANDLER); } out.append("  \"name\": ").append(q(name)).append(",\n  \"size\": ").append(size).append(",\n  \"dataType\": ").append(q(dataTypeSummary(resolved))).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }
        if(action.equals("patch-bytes")){ if(args.length<3){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"patch-bytes address hex required\"}");return;} Address a=currentProgram.getAddressFactory().getAddress(args[1]); if(a==null){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"invalid address\"}");return;} String hx=args[2].replaceAll("[^0-9A-Fa-f]",""); if((hx.length()%2)!=0){println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"hex must have even length\"}");return;} byte[] bytes=new byte[hx.length()/2]; for(int i=0;i<bytes.length;i++){bytes[i]=(byte)Integer.parseInt(hx.substring(i*2,i*2+2),16);} byte[] old=new byte[bytes.length]; currentProgram.getMemory().getBytes(a,old); if(!dry){ currentProgram.getMemory().setBytes(a,bytes); } StringBuilder oh=new StringBuilder(); for(byte b:old)oh.append(String.format("%02x",b&0xff)); out.append("  \"address\": ").append(q(a.toString())).append(",\n  \"oldHex\": ").append(q(oh.toString())).append(",\n  \"newHex\": ").append(q(hx.toLowerCase())).append(",\n  \"length\": ").append(bytes.length).append("\n}"); println("VEGVISIR_JSON:"+out.toString()); return; }
        println("VEGVISIR_JSON:{\"ok\":false,\"error\":\"unknown action\"}"); }
}
