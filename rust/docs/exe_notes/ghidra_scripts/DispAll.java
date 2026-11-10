import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.scalar.*;
import java.io.*;
import java.util.*;

// args: outfile ; per function, the set of operand displacements that are multiples of 16 in [0x100, 0x40000]
public class DispAll extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        PrintWriter pw = new PrintWriter(new FileWriter(a[0], false));
        Map<Long, TreeSet<Long>> hits = new TreeMap<>();
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        while (it.hasNext() && !monitor.isCancelled()) {
            Instruction in = it.next();
            String m = in.getMnemonicString().toLowerCase();
            if (!(m.startsWith("mov") || m.startsWith("vmov") || m.startsWith("lea"))) continue;
            for (int op = 0; op < in.getNumOperands(); op++) {
                for (Object o : in.getOpObjects(op)) {
                    if (o instanceof Scalar) {
                        long v = ((Scalar) o).getSignedValue();
                        if (v >= 0x100 && v <= 0x40000 && (v & 15) == 0) {
                            Function f = getFunctionContaining(in.getAddress());
                            if (f == null) continue;
                            hits.computeIfAbsent(f.getEntryPoint().getOffset(), k -> new TreeSet<>()).add(v);
                        }
                    }
                }
            }
        }
        for (Map.Entry<Long, TreeSet<Long>> e : hits.entrySet()) {
            StringBuilder sb = new StringBuilder(Long.toHexString(e.getKey()));
            for (long v : e.getValue()) sb.append(' ').append(Long.toHexString(v));
            pw.println(sb);
        }
        pw.close();
    }
}
