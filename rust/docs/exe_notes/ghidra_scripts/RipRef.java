import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;
import java.io.*;

// args: outfile target... ; scans executable blocks for rel32 operands that land on a target (end-of-operand based, +0..+4 slack)
public class RipRef extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        PrintWriter pw = new PrintWriter(new FileWriter(a[0], true));
        for (int i = 1; i < a.length; i++) {
            long target = Long.decode(a[i]);
            pw.println("==== " + a[i]);
            for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
                if (!b.isExecute() || !b.isInitialized()) continue;
                long s = b.getStart().getOffset();
                long n = b.getSize();
                byte[] buf = new byte[(int) n];
                b.getBytes(b.getStart(), buf);
                for (int o = 0; o + 4 <= n; o++) {
                    int rel = (buf[o] & 0xff) | ((buf[o + 1] & 0xff) << 8) | ((buf[o + 2] & 0xff) << 16) | ((buf[o + 3] & 0xff) << 24);
                    long end = s + o + 4;
                    for (int slack = 0; slack <= 5; slack++) {
                        if (end + slack + rel == target) {
                            Address at = b.getStart().add(o);
                            Function f = getFunctionContaining(at);
                            pw.println(at + " slack " + slack + " in " + (f == null ? "?" : f.getName()));
                        }
                    }
                }
            }
        }
        pw.close();
    }
}
