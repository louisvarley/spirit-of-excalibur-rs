// Recover functions in gaps, then classify leftover gap bytes as data.
// @category AI

import java.io.BufferedReader;
import java.io.FileReader;
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
import ghidra.program.model.data.ByteDataType;
import ghidra.program.model.data.DataUtilities;
import ghidra.program.model.data.DataUtilities.ClearDataMode;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.reloc.Relocation;
import ghidra.program.model.reloc.RelocationTable;
import ghidra.program.model.symbol.RefType;
import ghidra.program.model.symbol.Reference;

public class RecoverGaps extends GhidraScript {

    private static final Pattern IMM = Pattern.compile("0x([0-9a-fA-F]+)");
    private static final Pattern CMPAX = Pattern.compile(
            "\\bCMP\\s+AX,\\s*(0x[0-9a-fA-F]+)");

    private static class Gap {
        MemoryBlock block;
        Address start;
        int len;
        Gap(MemoryBlock b, Address s, int l) { block = b; start = s; len = l; }
        boolean contains(long off) {
            return off >= start.getOffset() && off < start.getOffset() + len;
        }
    }

    private Listing listing;
    private Memory memory;
    private List<Gap> gaps = new ArrayList<>();
    private Set<Long> gapStarts = new HashSet<>();
    private List<String[]> actions = new ArrayList<>();
    private Set<Long> recovered = new HashSet<>();

    @Override
    protected void run() throws Exception {
        listing = currentProgram.getListing();
        memory = currentProgram.getMemory();
        loadGaps("C:\\Users\\louis\\Documents\\GitHub\\spirit-of-excalibur-rs\\notes\\gaps_raw.tsv");
        println("ORIGINAL_GAPS=" + gaps.size());

        // 1. Known prologue candidates.
        String[] known = { "2036:0009", "2a6e:0009", "2d65:0001",
                "321b:0004", "321e:000c", "332f:000c", "3337:0007" };
        for (String a : known) {
            createAt(toAddr(a), "known-prologue");
        }

        // 2. Scan every gap for additional Turbo C prologues. This also splits
        //    the two large gaps when they contain several functions.
        for (Gap g : gaps) {
            for (int i = 0; i + 2 < g.len; i++) {
                Address a = g.start.add(i);
                try {
                    if ((memory.getByte(a) & 0xff) == 0x55
                            && (memory.getByte(a.add(1)) & 0xff) == 0x8b
                            && (memory.getByte(a.add(2)) & 0xff) == 0xec) {
                        createAt(a, "prologue-in-gap");
                    }
                }
                catch (Exception e) {
                    break;
                }
            }
        }

        // 3. Make sure the 1ba8 orphan switch has a containing function.
        createAt(toAddr("1ba8:3030"), "1ba8-gap-start");
        Address sw = toAddr("1ba8:324e");
        for (int back = 2; back < 2048; back++) {
            Address a = sw.subtract(back);
            try {
                if ((memory.getByte(a) & 0xff) == 0x55
                        && (memory.getByte(a.add(1)) & 0xff) == 0x8b
                        && (memory.getByte(a.add(2)) & 0xff) == 0xec) {
                    createAt(a, "1ba8-switch-prologue");
                    break;
                }
            }
            catch (Exception e) {
                break;
            }
        }

        // 4. Reference-based recovery inside the original gaps.
        Map<String, String> funcs = new HashMap<>();
        Set<String> codeSegs = new HashSet<>();
        FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
        while (fit.hasNext()) {
            Function f = fit.next();
            funcs.put(f.getEntryPoint().toString(), f.getName());
            codeSegs.add(f.getEntryPoint().toString().split(":")[0]);
        }

        Set<Long> candidates = new HashSet<>();
        // Direct call/jump and computed-jump references.
        InstructionIterator iit = listing.getInstructions(true);
        while (iit.hasNext()) {
            Instruction ins = iit.next();
            for (Reference r : ins.getReferencesFrom()) {
                if (r.getReferenceType().isCall() || r.getReferenceType().isJump()) {
                    addCandidate(candidates, r.getToAddress().getOffset(), "ref-" + r.getReferenceType());
                }
            }
        }
        // Relocation pairs: segment word plus adjacent/nearby offset word.
        RelocationTable rt = currentProgram.getRelocationTable();
        Iterator<Relocation> rit = rt.getRelocations();
        while (rit.hasNext()) {
            Relocation r = rit.next();
            Address a = r.getAddress();
            int segVal;
            try { segVal = memory.getShort(a) & 0xffff; }
            catch (Exception e) { continue; }
            String seg = String.format("%04x", segVal);
            if (!codeSegs.contains(seg)) continue;
            for (int d : new int[] { -2, 2 }) {
                Address w = a.add(d);
                try {
                    if (memory.getBlock(w) == null) continue;
                    int off = memory.getShort(w) & 0xffff;
                    addCandidate(candidates, (long) segVal * 16 + off, "reloc-adjacent");
                }
                catch (Exception e) { }
            }
            Instruction segIns = listing.getInstructionContaining(a);
            if (segIns != null && registerImmediate(segIns) != null
                    && Integer.parseInt(registerImmediate(segIns), 16) == segVal) {
                Instruction scan = segIns;
                for (int i = 0; i < 3; i++) {
                    scan = listing.getInstructionAfter(scan.getAddress());
                    if (scan == null) break;
                    String ov = registerImmediate(scan);
                    if (ov != null) {
                        addCandidate(candidates, (long) segVal * 16 + Integer.parseInt(ov, 16),
                                "reloc-movpush");
                        break;
                    }
                }
            }
        }
        println("REFERENCE_CANDIDATES=" + candidates.size());

        // Create functions at candidates that are still outside functions.
        List<Long> sorted = new ArrayList<>(candidates);
        sorted.sort(null);
        int made = 0;
        for (long off : sorted) {
            Address a = toAddr(off);
            if (a == null) continue;
            if (currentProgram.getFunctionManager().getFunctionContaining(a) != null) continue;
            if (createAt(a, "reference-candidate")) made++;
        }
        println("REFERENCE_FUNCTIONS_CREATED=" + made);

        // 5. Recompute remaining gaps after function creation, and mark
        //    unexplained bytes as data (leave any remaining prologue code alone).
        List<Gap> remaining = remainingGaps();
        int dataBytes = 0, dataRegions = 0;
        try (PrintWriter w = new PrintWriter(
                "C:\\Users\\louis\\Documents\\GitHub\\spirit-of-excalibur-rs\\notes\\classified_data_raw.tsv", "UTF-8")) {
            w.println("block\tstart\tlength\treason");
            for (Gap g : remaining) {
                int printableRun = 0, stringRuns = 0, farPtrs = 0, padding = 0;
                for (int i = 0; i < g.len; i++) {
                    Address a = g.start.add(i);
                    try {
                        int v = memory.getByte(a) & 0xff;
                        if (v >= 0x20 && v <= 0x7e) {
                            printableRun++;
                            if (printableRun == 4) stringRuns++;
                        }
                        else printableRun = 0;
                        if (v == 0x00 || v == 0xff || v == 0x90) padding++;
                        if (i + 4 <= g.len) {
                            int off = memory.getShort(a) & 0xffff;
                            int seg = memory.getShort(a.add(2)) & 0xffff;
                            String segs = String.format("%04x", seg);
                            if (codeSegs.contains(segs)) {
                                MemoryBlock tb = memory.getBlock(toAddr(segs + ":"
                                        + String.format("%04x", off)));
                                if (tb != null && tb.isExecute()) farPtrs++;
                            }
                        }
                    }
                    catch (Exception e) { break; }
                }
                String reason = null;
                if (stringRuns > 0) reason = "strings";
                else if (farPtrs >= 2) reason = "far-pointers";
                else if (padding > g.len * 0.6) reason = "padding";
                if (reason == null) {
                    w.println(g.block.getName() + "\t" + g.start + "\t" + g.len + "\tcode-like");
                    continue;
                }
                for (int i = 0; i < g.len; i++) {
                    Address a = g.start.add(i);
                    if (currentProgram.getFunctionManager().getFunctionContaining(a) != null) continue;
                    if (listing.getInstructionAt(a) != null) continue;
                    if (listing.getDefinedDataAt(a) != null) continue;
                    try {
                        DataUtilities.createData(currentProgram, a, new ByteDataType(), -1,
                                ClearDataMode.CLEAR_ALL_CONFLICT_DATA);
                        dataBytes++;
                    }
                    catch (Exception e) { }
                }
                dataRegions++;
                w.println(g.block.getName() + "\t" + g.start + "\t" + g.len + "\t" + reason);
            }
        }
        println("DATA_REGIONS_MARKED=" + dataRegions + " DATA_BYTES=" + dataBytes);

        try (PrintWriter w = new PrintWriter(
                "C:\\Users\\louis\\Documents\\GitHub\\spirit-of-excalibur-rs\\notes\\gap_recovery_raw.tsv", "UTF-8")) {
            w.println("action\taddress\tdetail");
            for (String[] x : actions) w.println(String.join("\t", x));
        }
    }

    private void loadGaps(String path) throws Exception {
        Map<String, MemoryBlock> blocks = new HashMap<>();
        for (MemoryBlock b : memory.getBlocks()) blocks.put(b.getName(), b);
        try (BufferedReader r = new BufferedReader(new FileReader(path))) {
            String line = r.readLine();
            while ((line = r.readLine()) != null) {
                String[] p = line.split("\t");
                if (p.length < 3) continue;
                MemoryBlock b = blocks.get(p[0]);
                if (b == null) continue;
                int gs, go;
                String[] so = p[1].split(":");
                gs = Integer.parseInt(so[0], 16);
                go = Integer.parseInt(so[1], 16);
                int bs, bo;
                String[] sb = b.getStart().toString().split(":");
                bs = Integer.parseInt(sb[0], 16);
                bo = Integer.parseInt(sb[1], 16);
                int realOff = gs * 16 + go - bs * 16;
                Address a = b.getStart().add(realOff - bo);
                Gap g = new Gap(b, a, Integer.parseInt(p[2]));
                gaps.add(g);
                gapStarts.add(a.getOffset());
            }
        }
    }

    private void addCandidate(Set<Long> set, long off, String why) {
        for (Gap g : gaps) {
            if (g.contains(off)) {
                set.add(off);
                return;
            }
        }
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
            if (comma < 0 || space < 0) return null;
            String dst = t.substring(space + 1, comma).trim();
            if (dst.contains("[")) return null;
            Matcher im = IMM.matcher(t.substring(comma));
            return im.find() ? im.group(1) : null;
        }
        return null;
    }

    private boolean createAt(Address a, String why) {
        if (a == null) return false;
        if (currentProgram.getFunctionManager().getFunctionAt(a) != null) return false;
        try {
            disassemble(a);
            Function f = createFunction(a, null);
            if (f != null) {
                actions.add(new String[] { why, a.toString(), "created " + f.getName() });
                recovered.add(a.getOffset());
                return true;
            }
        }
        catch (Exception e) {
            actions.add(new String[] { why, a.toString(), "failed: " + e.getMessage() });
        }
        return false;
    }

    private List<Gap> remainingGaps() {
        List<Gap> out = new ArrayList<>();
        for (MemoryBlock b : memory.getBlocks()) {
            if (!b.isExecute()) continue;
            long start = -1;
            int len = 0;
            for (long off = b.getStart().getOffset(); off <= b.getEnd().getOffset(); off++) {
                Address a = toAddr(off);
                boolean covered = currentProgram.getFunctionManager().getFunctionContaining(a) != null
                        || listing.getInstructionAt(a) != null
                        || listing.getDefinedDataAt(a) != null;
                if (!covered) {
                    if (start < 0) { start = off; len = 1; }
                    else len++;
                }
                else if (start >= 0) {
                    if (len >= 16) out.add(new Gap(b,
                            b.getStart().add(start - b.getStart().getOffset()), len));
                    start = -1; len = 0;
                }
            }
            if (start >= 0 && len >= 16) {
                out.add(new Gap(b, b.getStart().add(start - b.getStart().getOffset()), len));
            }
        }
        return out;
    }
}
