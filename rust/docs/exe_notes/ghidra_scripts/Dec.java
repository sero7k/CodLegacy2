import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.util.*;

// args: outfile cmd... ; commands: dec:ADDR  xr:ADDR  str:SUBSTR  fn:ADDR(function containing)
public class Dec extends GhidraScript {
    DecompInterface di;
    PrintWriter pw;
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        pw = new PrintWriter(new FileWriter(a[0], true));
        di = new DecompInterface();
        di.openProgram(currentProgram);
        for (int i = 1; i < a.length; i++) {
            String c = a[i];
            int k = c.indexOf(':');
            String cmd = c.substring(0, k), arg = c.substring(k + 1);
            pw.println("==== " + c);
            try {
                if (cmd.equals("dec")) dec(toAddr(arg));
                else if (cmd.equals("xr")) xr(toAddr(arg));
                else if (cmd.equals("str")) str(arg);
                else if (cmd.equals("bytes")) bytes(arg);
                else if (cmd.equals("refmap")) { String[] p = arg.split("[,_]"); long s = Long.decode(p[0]), e = Long.decode(p[1]);
                    ghidra.program.model.symbol.ReferenceManager rm = currentProgram.getReferenceManager();
                    AddressIterator ai = rm.getReferenceDestinationIterator(toAddr(s), true);
                    while (ai.hasNext()) { Address ad = ai.next(); if (ad.getOffset() >= e) break;
                        ReferenceIterator it = rm.getReferencesTo(ad); StringBuilder sb = new StringBuilder();
                        while (it.hasNext()) { Reference r = it.next(); Function f = getFunctionContaining(r.getFromAddress());
                            sb.append(r.getReferenceType().isWrite() ? "W:" : "R:").append(f == null ? "?" : f.getEntryPoint().toString()).append(' '); }
                        pw.println(ad + " " + sb); } }
                else if (cmd.equals("fconst")) {
                    int want = Float.floatToIntBits(Float.parseFloat(arg));
                    for (ghidra.program.model.mem.MemoryBlock b : currentProgram.getMemory().getBlocks()) {
                        if (!b.isInitialized() || b.isExecute()) continue;
                        Address s = b.getStart(); long n = b.getSize();
                        for (long o = 0; o + 4 <= n; o += 4) {
                            Address ad = s.add(o);
                            if (currentProgram.getMemory().getInt(ad) == want) {
                                pw.println("const @" + ad + " in " + b.getName());
                                xr(ad);
                            }
                        }
                    }
                }
                else if (cmd.equals("imm")) {
                    long want = Long.decode(arg);
                    InstructionIterator it = currentProgram.getListing().getInstructions(true);
                    while (it.hasNext()) { Instruction in = it.next();
                        for (int op = 0; op < in.getNumOperands(); op++) for (Object o : in.getOpObjects(op))
                            if (o instanceof ghidra.program.model.scalar.Scalar && ((ghidra.program.model.scalar.Scalar) o).getUnsignedValue() == (want & 0xffffffffL)) {
                                Function f = getFunctionContaining(in.getAddress());
                                pw.println(in.getAddress() + " " + in + " in " + (f == null ? "?" : f.getName()));
                            }
                    }
                }
                else if (cmd.equals("xrr")) { String[] p = arg.split("[,_]"); long s = Long.decode(p[0]), e = Long.decode(p[1]);
                    for (long x = s; x < e; x += 1) { Address ad = toAddr(x); ReferenceIterator it = currentProgram.getReferenceManager().getReferencesTo(ad);
                        while (it.hasNext()) { Reference r = it.next(); Function f = getFunctionContaining(r.getFromAddress()); pw.println(ad + " <- " + r.getFromAddress() + " in " + (f==null?"?":f.getName())); } } }
                else if (cmd.equals("callers")) callers(toAddr(arg));
            } catch (Exception e) { pw.println("ERR " + e); }
            pw.flush();
        }
        pw.close();
    }
    void dec(Address ad) throws Exception {
        Function f = getFunctionContaining(ad);
        if (f == null) { pw.println("no function at " + ad); return; }
        DecompileResults r = di.decompileFunction(f, 120, monitor);
        pw.println("// function " + f.getName() + " @ " + f.getEntryPoint() + " size " + f.getBody().getNumAddresses());
        if (r.getDecompiledFunction() != null) pw.println(r.getDecompiledFunction().getC());
        else pw.println("decompile failed " + r.getErrorMessage());
    }
    void xr(Address ad) {
        ReferenceIterator it = currentProgram.getReferenceManager().getReferencesTo(ad);
        while (it.hasNext()) {
            Reference r = it.next();
            Function f = getFunctionContaining(r.getFromAddress());
            pw.println(r.getFromAddress() + " " + r.getReferenceType() + " in " + (f == null ? "?" : f.getName() + "@" + f.getEntryPoint()));
        }
    }
    void callers(Address ad) {
        Function f = getFunctionAt(ad);
        if (f == null) f = getFunctionContaining(ad);
        if (f == null) return;
        xr(f.getEntryPoint());
    }
    void str(String sub) throws Exception {
        Data d = getFirstData();
        while (d != null && !monitor.isCancelled()) {
            if (d.hasStringValue()) {
                Object v = d.getValue();
                if (v != null && v.toString().contains(sub)) {
                    pw.println(d.getAddress() + " \"" + v + "\"");
                    xr(d.getAddress());
                }
            }
            d = getDataAfter(d);
        }
    }
    void bytes(String arg) throws Exception {
        String[] p = arg.split("[,_]");
        Address ad = toAddr(p[0]);
        int n = Integer.parseInt(p[1]);
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < n; i++) {
            sb.append(String.format("%02x", getByte(ad.add(i)) & 0xff));
            if (i % 16 == 15) { pw.println(ad.add(i - 15) + ": " + sb); sb.setLength(0); }
        }
        if (sb.length() > 0) pw.println(sb);
    }
}
