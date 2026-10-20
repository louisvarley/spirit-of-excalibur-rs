// Coverage/overlap/gap report plus Turbo C prologue scan in gaps.
// @category AI

import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressIterator;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.Reference;

public class CoverageReport extends GhidraScript {

    private static final Pattern P = Pattern.compile(
            "\\bJMP\\s+word ptr CS:\\[([A-Z0-9]+)\\s*\\+\\s*(0x[0-9a-fA-F]+)\\]");
    private static final Pattern MOVI = Pattern.compile(
            "\\bMOV\\s+(CX|BX),\\s*(0x[0-9a-fA-F]+)");

    @Override
    protected void run() throws Exception {
        String base = "C:\\Users\\louis\\Documents\\GitHub\\spirit-of-excalibur-rs\\notes\\";
        AddressSet funcSet = new AddressSet();
        List<Function> funcs = new ArrayList<>();
        FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
        while (fit.hasNext()) {
            Function f = fit.next();
            funcs.add(f);
            funcSet.add(f.getBody());
        }

        AddressSet insSet = new AddressSet();
        InstructionIterator allIns = currentProgram.getListing().getInstructions(true);
        while (allIns.hasNext()) {
            Instruction in = allIns.next();
            insSet.add(in.getMinAddress(), in.getMaxAddress());
        }
        AddressSet dataSet = new AddressSet();
        DataIterator dit = currentProgram.getListing().getDefinedData(true);
        while (dit.hasNext()) {
            Data d = dit.next();
            dataSet.add(d.getMinAddress(), d.getMaxAddress());
        }
        AddressSet tableSet = new AddressSet();
        AddressSet allCovered = new AddressSet(funcSet);
        allCovered.add(insSet);
        allCovered.add(dataSet);
        int switchTables = 0;
        InstructionIterator iit = currentProgram.getListing().getInstructions(true);
        while (iit.hasNext()) {
            Instruction ins = iit.next();
            Matcher m = P.matcher(ins.toString());
            if (!m.find()) {
                continue;
            }
            int disp = Integer.parseInt(m.group(2).substring(2), 16);
            int n = -1, tableOff = -1;
            boolean cmpLoop = false;
            Instruction cur = currentProgram.getListing().getInstructionBefore(ins.getAddress());
            for (int i = 0; i < 16 && cur != null; i++) {
                String t = cur.toString();
                if (t.startsWith("CMP AX,word ptr CS:[BX]")) {
                    cmpLoop = true;
                }
                Matcher mm = MOVI.matcher(t);
                if (mm.find()) {
                    if (mm.group(1).equals("CX") && n < 0) {
                        n = Integer.parseInt(mm.group(2).substring(2), 16);
                    }
                    else if (mm.group(1).equals("BX") && tableOff < 0) {
                        tableOff = Integer.parseInt(mm.group(2).substring(2), 16);
                    }
                }
                cur = currentProgram.getListing().getInstructionBefore(cur.getAddress());
            }
            MemoryBlock b = currentProgram.getMemory().getBlock(ins.getAddress());
            if (b == null) {
                continue;
            }
            String seg = b.getStart().toString().split(":")[0];
            if (n > 0 && tableOff >= 0 && cmpLoop && disp == 2 * n) {
                tableSet.add(inBlock(b, tableOff), inBlock(b, tableOff + 4 * n - 1));
                switchTables++;
            }
            else {
                int insOff = Integer.parseInt(ins.getAddress().toString().split(":")[1], 16);
                int tableOff2 = insOff + ins.getLength();
                Address tab = inBlock(b, tableOff2);
                int entry0;
                try {
                    entry0 = currentProgram.getMemory().getShort(tab) & 0xffff;
                }
                catch (Exception e) {
                    continue;
                }
                int formulaN = (entry0 - tableOff2) / 2;
                int bound = findBound(ins.getAddress());
                int useN = -1;
                if (bound > 0 && validEntries(b, tableOff2, bound + 1)) {
                    useN = bound + 1;
                }
                else if (formulaN > 0 && formulaN <= 512 && validEntries(b, tableOff2, formulaN)) {
                    useN = formulaN;
                }
                if (useN > 0) {
                    tableSet.add(inBlock(b, tableOff2), inBlock(b, tableOff2 + 2 * useN - 1));
                    switchTables++;
                }
            }
        }

        allCovered.add(tableSet);

        try (PrintWriter cov = new PrintWriter(base + "coverage_raw.tsv", "UTF-8");
                PrintWriter gaps = new PrintWriter(base + "gaps_raw.tsv", "UTF-8");
                PrintWriter cand = new PrintWriter(base + "gap_candidates_raw.tsv", "UTF-8");
                PrintWriter ov = new PrintWriter(base + "overlaps_raw.tsv", "UTF-8")) {
            cov.println("block\tstart\tend\tsize\tfunc_bytes\ttable_bytes\tcode_bytes\tdata_bytes\tcovered_bytes\tgap_count\tgap_bytes\toverlap_pairs");
            gaps.println("block\tgap_start\tgap_len\tcandidates");
            cand.println("block\tgap_start\tgap_len\tcandidate");
            ov.println("func1\tfunc2\tbytes\tblock");

            int totalGaps = 0, totalCands = 0, totalOverlaps = 0;
            for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
                if (!b.isExecute()) {
                    continue;
                }
                AddressSet bs = new AddressSet(b.getStart(), b.getEnd());
                long funcBytes = bs.intersect(funcSet).getNumAddresses();
                long tableBytes = bs.intersect(tableSet).getNumAddresses();
                long codeBytes = bs.intersect(insSet).getNumAddresses();
                long dataBytes = bs.intersect(dataSet).getNumAddresses();
                long coveredBytes = bs.intersect(allCovered).getNumAddresses();
                List<Address> gapStarts = new ArrayList<>();
                List<Integer> gapLens = new ArrayList<>();
                Address runStart = null;
                int runLen = 0;
                AddressIterator ai = bs.getAddresses(true);
                while (ai.hasNext()) {
                    Address a = ai.next();
                    boolean covered = allCovered.contains(a);
                    if (!covered) {
                        if (runStart == null) {
                            runStart = a;
                            runLen = 1;
                        }
                        else {
                            runLen++;
                        }
                    }
                    else if (runStart != null) {
                        if (runLen >= 16) {
                            gapStarts.add(runStart);
                            gapLens.add(runLen);
                        }
                        runStart = null;
                        runLen = 0;
                    }
                }
                if (runStart != null && runLen >= 16) {
                    gapStarts.add(runStart);
                    gapLens.add(runLen);
                }

                int gapBytes = 0;
                for (int i = 0; i < gapStarts.size(); i++) {
                    Address gs = gapStarts.get(i);
                    int gl = gapLens.get(i);
                    gapBytes += gl;
                    totalGaps++;
                    StringBuilder cs = new StringBuilder();
                    for (int k = 0; k < gl - 2; k++) {
                        Address a = gs.add(k);
                        try {
                            if ((currentProgram.getMemory().getByte(a) & 0xff) == 0x55
                                    && (currentProgram.getMemory().getByte(a.add(1)) & 0xff) == 0x8b
                                    && (currentProgram.getMemory().getByte(a.add(2)) & 0xff) == 0xec) {
                                cand.println(b.getName() + "\t" + gs + "\t" + gl + "\t" + a);
                                if (cs.length() > 0) {
                                    cs.append(",");
                                }
                                cs.append(a);
                                totalCands++;
                            }
                        }
                        catch (Exception e) {
                            break;
                        }
                    }
                    gaps.println(b.getName() + "\t" + gs + "\t" + gl + "\t" + cs);
                }

                int overlapPairs = 0;
                List<Function> inBlock = new ArrayList<>();
                for (Function f : funcs) {
                    if (bs.intersects(f.getBody())) {
                        inBlock.add(f);
                    }
                }
                for (int i = 0; i < inBlock.size(); i++) {
                    for (int j = i + 1; j < inBlock.size(); j++) {
                        AddressSet inter = new AddressSet(inBlock.get(i).getBody());
                        inter = inter.intersect(inBlock.get(j).getBody());
                        long n = inter.getNumAddresses();
                        if (n > 0) {
                            overlapPairs++;
                            totalOverlaps++;
                            ov.println(inBlock.get(i).getName() + "\t" + inBlock.get(j).getName()
                                    + "\t" + n + "\t" + b.getName());
                        }
                    }
                }
                cov.println(b.getName() + "\t" + b.getStart() + "\t" + b.getEnd() + "\t"
                        + b.getSize() + "\t" + funcBytes + "\t" + tableBytes + "\t"
                        + codeBytes + "\t" + dataBytes + "\t" + coveredBytes + "\t"
                        + gapStarts.size() + "\t" + gapBytes + "\t" + overlapPairs);
            }
            println("COVERAGE blocksDone switchTables=" + switchTables + " gaps=" + totalGaps
                    + " gapCandidates=" + totalCands + " overlapPairs=" + totalOverlaps);
        }
    }

    private Address inBlock(MemoryBlock b, int off) {
        int bOff = Integer.parseInt(b.getStart().toString().split(":")[1], 16);
        return b.getStart().add(off - bOff);
    }

    private int findBound(Address switchAddr) {
        Instruction cur = currentProgram.getListing().getInstructionBefore(switchAddr);
        java.util.regex.Pattern cmp = java.util.regex.Pattern.compile(
                "\\bCMP\\s+AX,\\s*(0x[0-9a-fA-F]+)");
        for (int i = 0; i < 12 && cur != null; i++) {
            Matcher m = cmp.matcher(cur.toString());
            if (m.find()) {
                return Integer.parseInt(m.group(1).substring(2), 16);
            }
            cur = currentProgram.getListing().getInstructionBefore(cur.getAddress());
        }
        return -1;
    }

    private boolean validEntries(MemoryBlock b, int tableOff, int n) {
        if (n < 1 || n > 512) {
            return false;
        }
        for (int i = 0; i < n; i++) {
            int v;
            try {
                v = currentProgram.getMemory().getShort(inBlock(b, tableOff + i * 2)) & 0xffff;
            }
            catch (Exception e) {
                return false;
            }
            Address t = inBlock(b, v);
            if (b.getStart().compareTo(t) > 0 || b.getEnd().compareTo(t) < 0) {
                return false;
            }
        }
        return true;
    }
}
