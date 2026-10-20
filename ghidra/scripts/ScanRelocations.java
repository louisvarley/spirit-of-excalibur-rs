// Scan the MZ relocation table for segment-valued words that point at real code
// segments, then pair them with adjacent words / MOV-PUSH immediates that are
// exact function entries. Adds DATA references from the offset word/instruction.
// @category AI

import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.reloc.Relocation;
import ghidra.program.model.reloc.RelocationTable;
import ghidra.program.model.symbol.RefType;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceManager;
import ghidra.program.model.symbol.SourceType;

public class ScanRelocations extends GhidraScript {

    private static final Pattern IMM = Pattern.compile("0x([0-9a-fA-F]+)");

    @Override
    protected void run() throws Exception {
        Listing listing = currentProgram.getListing();
        Memory mem = currentProgram.getMemory();
        ReferenceManager refs = currentProgram.getReferenceManager();

        Map<String, String> funcs = new HashMap<>();
        Set<String> codeSegs = new HashSet<>();
        FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
        while (fit.hasNext()) {
            Function f = fit.next();
            String e = f.getEntryPoint().toString();
            funcs.put(e, f.getName());
            codeSegs.add(e.split(":")[0]);
        }

        RelocationTable rt = currentProgram.getRelocationTable();
        Iterator<Relocation> it = rt.getRelocations();
        List<String[]> rows = new ArrayList<>();
        Set<String> seen = new HashSet<>();
        int relocs = 0, codeRelocs = 0, hits = 0;

        while (it.hasNext() && !monitor.isCancelled()) {
            Relocation r = it.next();
            relocs++;
            Address a = r.getAddress();
            int segVal;
            try {
                segVal = mem.getShort(a) & 0xffff;
            }
            catch (Exception e) {
                continue;
            }
            String seg = String.format("%04x", segVal);
            if (!codeSegs.contains(seg)) {
                continue;
            }
            codeRelocs++;

            // Adjacent words: far pointers and CALLF immediates.
            int[] deltas = { -2, 2 };
            for (int d : deltas) {
                Address w = a.add(d);
                try {
                    if (mem.getBlock(w) == null) {
                        continue;
                    }
                    int off = mem.getShort(w) & 0xffff;
                    String target = seg + ":" + String.format("%04x", off);
                    if (!funcs.containsKey(target)) {
                        continue;
                    }
                    Instruction ins = listing.getInstructionContaining(w);
                    if (ins == null) {
                        ins = listing.getInstructionContaining(a);
                    }
                    String kind = ins == null ? "data" : "code";
                    String dir = d < 0 ? "before" : "after";
                    addHit(rows, seen, refs, w, target, funcs.get(target), kind, dir,
                            a.toString(), seg);
                    hits++;
                }
                catch (Exception e) {
                    // ignore
                }
            }

            // Strict MOV/PUSH immediate pairs: the segment immediate must be a
            // register MOV or PUSH, and the offset must be a register MOV/PUSH
            // within a few instructions. Memory stores are handled only by the
            // adjacent-word check above.
            Instruction segIns = listing.getInstructionContaining(a);
            if (segIns != null && scanCodePair(segIns, seg, segVal, a, funcs, rows, seen, refs)) {
                hits++;
            }
        }

        String out = "C:\\Users\\louis\\Documents\\GitHub\\spirit-of-excalibur-rs\\notes\\reloc_pointers_raw.tsv";
        try (PrintWriter w = new PrintWriter(out, "UTF-8")) {
            w.println("reloc_addr\tsegment\tword_addr\toffset\ttarget\tfunction\tkind\tdirection");
            for (String[] x : rows) {
                w.println(String.join("\t", x));
            }
        }
        println("RELOC_SCAN relocs=" + relocs + " codeSegmentRelocs=" + codeRelocs
                + " hits=" + hits + " uniqueRows=" + rows.size());
    }


    private boolean scanCodePair(Instruction segIns, String seg, int segVal, Address relocAddr,
            Map<String, String> funcs, List<String[]> rows, Set<String> seen,
            ReferenceManager refs) {
        String sv = registerImmediate(segIns);
        if (sv == null || Integer.parseInt(sv, 16) != segVal) {
            return false;
        }
        Instruction scan = segIns;
        for (int i = 0; i < 3; i++) {
            scan = currentProgram.getListing().getInstructionAfter(scan.getAddress());
            if (scan == null) {
                break;
            }
            String ov = registerImmediate(scan);
            if (ov != null) {
                int val = Integer.parseInt(ov, 16);
                String target = seg + ":" + String.format("%04x", val);
                if (funcs.containsKey(target)) {
                    addHit(rows, seen, refs, scan.getAddress(), target, funcs.get(target),
                            "code-pair", "after", relocAddr.toString(), seg);
                    return true;
                }
            }
        }
        scan = segIns;
        for (int i = 0; i < 3; i++) {
            scan = currentProgram.getListing().getInstructionBefore(scan.getAddress());
            if (scan == null) {
                break;
            }
            String ov = registerImmediate(scan);
            if (ov != null) {
                int val = Integer.parseInt(ov, 16);
                String target = seg + ":" + String.format("%04x", val);
                if (funcs.containsKey(target)) {
                    addHit(rows, seen, refs, scan.getAddress(), target, funcs.get(target),
                            "code-pair", "before", relocAddr.toString(), seg);
                    return true;
                }
            }
        }
        return false;
    }

    private String registerImmediate(Instruction ins) {
        String m = ins.getMnemonicString().toUpperCase();
        String t = ins.toString();
        if (m.equals("PUSH")) {
            Matcher im = IMM.matcher(t);
            return im.find() ? im.group(1) : null;
        }
        if (m.equals("MOV")) {
            int comma = t.indexOf(',');
            int space = t.indexOf(' ');
            if (comma < 0 || space < 0) {
                return null;
            }
            String dst = t.substring(space + 1, comma).trim();
            if (dst.contains("[")) {
                return null;
            }
            Matcher im = IMM.matcher(t.substring(comma));
            return im.find() ? im.group(1) : null;
        }
        return null;
    }

    private void addHit(List<String[]> rows, Set<String> seen, ReferenceManager refs,
            Address from, String target, String fn, String kind, String dir,
            String relocAddr, String seg) {
        String key = from + "|" + target;
        if (!seen.add(key)) {
            return;
        }
        try {
            Address to = toAddr(target);
            boolean exists = false;
            for (Reference r : refs.getReferencesFrom(from)) {
                if (r.getToAddress().equals(to)) {
                    exists = true;
                    break;
                }
            }
            if (!exists) {
                refs.addMemoryReference(from, to, RefType.DATA, SourceType.USER_DEFINED, 0);
            }
        }
        catch (Exception e) {
            // ignore bad addresses
        }
        rows.add(new String[] { relocAddr, seg, from.toString(), target.split(":")[1],
                target, fn, kind, dir });
    }
}
