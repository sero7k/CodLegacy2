import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.scalar.*;
import java.io.*;
import java.util.*;

// args: outfile disp1 disp2 ...  (hex or dec) ; lists, per function, which displacements appear in operands
public class FindDisp extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        PrintWriter pw = new PrintWriter(new FileWriter(a[0], false));
        Set<Long> want = new HashSet<>();
        for (int i = 1; i < a.length; i++) want.add(Long.decode(a[i]));
        Map<Function, Map<Long, Integer>> hits = new TreeMap<>((x, y) -> x.getEntryPoint().compareTo(y.getEntryPoint()));
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        while (it.hasNext() && !monitor.isCancelled()) {
            Instruction in = it.next();
            for (int op = 0; op < in.getNumOperands(); op++) {
                for (Object o : in.getOpObjects(op)) {
                    if (o instanceof Scalar) {
                        long v = ((Scalar) o).getSignedValue();
                        if (want.contains(v)) {
                            Function f = getFunctionContaining(in.getAddress());
                            if (f == null) continue;
                            hits.computeIfAbsent(f, k -> new TreeMap<>()).merge(v, 1, Integer::sum);
                        }
                    }
                }
            }
        }
        for (Map.Entry<Function, Map<Long, Integer>> e : hits.entrySet()) {
            pw.println(e.getKey().getEntryPoint() + " " + e.getKey().getName() + " size=" + e.getKey().getBody().getNumAddresses() + " " + e.getValue());
        }
        pw.close();
    }
}
