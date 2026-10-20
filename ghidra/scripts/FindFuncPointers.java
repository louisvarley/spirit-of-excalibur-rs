// Scan initialized DGROUP (3364:0000-81bb) for 4-byte far pointers to function entries.
// @category AI

import java.io.PrintWriter;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.mem.MemoryBlock;

public class FindFuncPointers extends GhidraScript {

    @Override
    protected void run() throws Exception {
        MemoryBlock dgroup = currentProgram.getMemory().getBlock(toAddr("3364:0000"));
        Set<String> codeSegs = new HashSet<>();
        Map<String, String> funcs = new HashMap<>();
        FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
        while (fit.hasNext()) {
            Function f = fit.next();
            String e = f.getEntryPoint().toString();
            funcs.put(e, f.getName());
            codeSegs.add(e.split(":")[0]);
        }

        String outPath = "C:\\Users\\louis\\Documents\\GitHub\\spirit-of-excalibur-rs\\notes\\func_pointers_raw.tsv";
        int count = 0;
        try (PrintWriter w = new PrintWriter(outPath, "UTF-8")) {
            w.println("ds_offset\ttarget\tfunction");
            long size = dgroup.getSize();
            for (long off = 0; off + 4 <= size; off += 1) {
                Address a = dgroup.getStart().add(off);
                int po = currentProgram.getMemory().getShort(a) & 0xffff;
                int ps = currentProgram.getMemory().getShort(a.add(2)) & 0xffff;
                String seg = String.format("%04x", ps);
                if (!codeSegs.contains(seg)) {
                    continue;
                }
                String target = seg + ":" + String.format("%04x", po);
                String fn = funcs.get(target);
                if (fn == null) {
                    continue;
                }
                w.println(String.format("%04x", off) + "\t" + target + "\t" + fn);
                count++;
            }
        }
        println("FUNC_POINTERS count=" + count + " codeSegs=" + codeSegs.size()
                + " functions=" + funcs.size());
    }
}
