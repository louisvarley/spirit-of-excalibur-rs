// Exports per-function disassembly, decompiled C, xrefs and global tables as text files
// so an AI agent can work from plain files instead of the Ghidra GUI.
// Args (headless): [outputDir]. In the GUI you will be asked for the directory.
// @category AI

import java.io.File;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolIterator;

public class ExportForAI extends GhidraScript {

    private Listing listing;
    private DecompInterface ifc;
    private int decompFailures = 0;
    private final List<String> failedFunctions = new ArrayList<>();

    private DecompInterface newDecompiler() {
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        return d;
    }

    // A decompiler exception can leave the process unusable, so replace it.
    private void resetDecompiler() {
        try {
            ifc.dispose();
        }
        catch (Throwable ignored) {
        }
        ifc = newDecompiler();
    }

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        File outDir = (args.length > 0) ? new File(args[0])
                : askDirectory("Export directory", "Select");
        File fnDir = new File(outDir, "functions");
        fnDir.mkdirs();
        listing = currentProgram.getListing();

        ifc = newDecompiler();

        try (PrintWriter index = new PrintWriter(new File(outDir, "index.tsv"), "UTF-8");
                PrintWriter calls = new PrintWriter(new File(outDir, "callgraph.tsv"), "UTF-8")) {

            index.println("address\tname\tsize_bytes\tinstructions\tcallers\tcallees\tthunk\tfile");
            calls.println("caller\tcaller_addr\tcallee\tcallee_addr");

            FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
            int count = 0;
            while (fit.hasNext() && !monitor.isCancelled()) {
                Function f = fit.next();
                exportFunction(f, fnDir, index, calls);
                count++;
                if (count % 50 == 0) {
                    monitor.setMessage("Exported " + count + " functions");
                }
            }
            println("Exported " + count + " functions (" + decompFailures
                    + " failed to decompile; disassembly still exported for those)");
            if (!failedFunctions.isEmpty()) {
                println("Decompile failures: " + String.join(", ", failedFunctions));
            }
        }
        finally {
            ifc.dispose();
        }

        exportSegments(outDir);
        exportStrings(outDir);
        exportUserSymbols(outDir);
        println("Done. Output in " + outDir.getAbsolutePath());
    }

    private void exportFunction(Function f, File fnDir, PrintWriter index,
            PrintWriter calls) throws Exception {

        Set<Function> callers = f.getCallingFunctions(monitor);
        Set<Function> callees = f.getCalledFunctions(monitor);

        List<String> asm = new ArrayList<>();
        Map<String, String> dataRefs = new TreeMap<>();
        int icount = 0;
        InstructionIterator ii = listing.getInstructions(f.getBody(), true);
        while (ii.hasNext()) {
            Instruction ins = ii.next();
            icount++;
            String comment = branchComment(ins);
            String line = String.format("%s  %-22s %s", ins.getAddress(), bytesHex(ins), ins);
            if (!comment.isEmpty()) {
                line += "  ; " + comment;
            }
            asm.add(line);
            for (Reference r : ins.getReferencesFrom()) {
                if (r.getReferenceType().isData() && r.getToAddress().isMemoryAddress()) {
                    dataRefs.put(r.getToAddress().toString(), describe(r.getToAddress()));
                }
            }
        }

        String c;
        boolean decompiled = false;
        try {
            DecompileResults res = ifc.decompileFunction(f, 60, monitor);
            if (res != null && res.decompileCompleted() && res.getDecompiledFunction() != null) {
                c = res.getDecompiledFunction().getC();
                decompiled = true;
            }
            else {
                c = "/* decompile failed: " + (res == null ? "null" : res.getErrorMessage()) + " */";
            }
        }
        catch (Throwable t) {
            c = "/* decompile failed: " + t + " */";
        }
        if (!decompiled) {
            decompFailures++;
            failedFunctions.add(f.getEntryPoint().toString());
            // Restart the decompiler so one bad function cannot poison later ones.
            resetDecompiler();
        }

        String fileName = key(f.getEntryPoint()) + "_" + safe(f.getName()) + ".md";
        try (PrintWriter w = new PrintWriter(new File(fnDir, fileName), "UTF-8")) {
            w.println("# " + f.getName() + " @ " + f.getEntryPoint());
            w.println();
            w.println("- signature: `" + f.getPrototypeString(false, false) + "`");
            w.println("- calling convention: " + f.getCallingConventionName());
            w.println("- stack purge size: " + f.getStackPurgeSize());
            w.println("- body: " + f.getBody().getMinAddress() + " - " + f.getBody().getMaxAddress()
                    + " (" + f.getBody().getNumAddresses() + " bytes)");
            w.println("- thunk: " + f.isThunk());
            if (f.getComment() != null) {
                w.println("- comment: " + f.getComment());
            }
            w.println();
            w.println("## Callers");
            for (Function g : callers) {
                w.println("- " + g.getName() + " @ " + g.getEntryPoint());
            }
            w.println();
            w.println("## Callees");
            for (Function g : callees) {
                w.println("- " + g.getName() + " @ " + g.getEntryPoint());
            }
            w.println();
            w.println("## Referenced data and strings");
            for (Map.Entry<String, String> e : dataRefs.entrySet()) {
                w.println("- " + e.getKey() + "  " + e.getValue());
            }
            w.println();
            w.println("## Decompiled (Ghidra, treat as a hint, disassembly is ground truth)");
            w.println("```c");
            w.println(c);
            w.println("```");
            w.println();
            w.println("## Disassembly");
            w.println("```");
            for (String line : asm) {
                w.println(line);
            }
            w.println("```");
        }

        for (Function g : callees) {
            calls.println(f.getName() + "\t" + f.getEntryPoint() + "\t" + g.getName() + "\t"
                    + g.getEntryPoint());
        }
        index.println(f.getEntryPoint() + "\t" + f.getName() + "\t" + f.getBody().getNumAddresses()
                + "\t" + icount + "\t" + callers.size() + "\t" + callees.size() + "\t"
                + f.isThunk() + "\t" + fileName);
    }

    private void exportSegments(File outDir) throws Exception {
        try (PrintWriter w = new PrintWriter(new File(outDir, "segments.tsv"), "UTF-8")) {
            w.println("name\tstart\tend\tsize\tperms\tinitialized");
            for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
                w.println(b.getName() + "\t" + b.getStart() + "\t" + b.getEnd() + "\t" + b.getSize()
                        + "\t" + (b.isRead() ? "r" : "-") + (b.isWrite() ? "w" : "-")
                        + (b.isExecute() ? "x" : "-") + "\t" + b.isInitialized());
            }
        }
    }

    private void exportStrings(File outDir) throws Exception {
        try (PrintWriter w = new PrintWriter(new File(outDir, "strings.tsv"), "UTF-8")) {
            w.println("address\tvalue");
            DataIterator di = listing.getDefinedData(true);
            while (di.hasNext()) {
                Data d = di.next();
                if (d.hasStringValue()) {
                    w.println(d.getAddress() + "\t" + d.getDefaultValueRepresentation());
                }
            }
        }
    }

    // Everything you have renamed in Ghidra, so renames are visible as a diffable file.
    private void exportUserSymbols(File outDir) throws Exception {
        try (PrintWriter w = new PrintWriter(new File(outDir, "user_symbols.tsv"), "UTF-8")) {
            w.println("address\tname\ttype");
            SymbolIterator si = currentProgram.getSymbolTable().getAllSymbols(true);
            while (si.hasNext()) {
                Symbol s = si.next();
                if (s.getSource() == SourceType.USER_DEFINED) {
                    w.println(s.getAddress() + "\t" + s.getName() + "\t" + s.getSymbolType());
                }
            }
        }
    }

    private String describe(Address to) {
        StringBuilder sb = new StringBuilder();
        Symbol s = currentProgram.getSymbolTable().getPrimarySymbol(to);
        if (s != null) {
            sb.append(s.getName());
        }
        Data d = listing.getDataAt(to);
        if (d != null) {
            if (d.hasStringValue()) {
                sb.append(" = ").append(d.getDefaultValueRepresentation());
            }
            else {
                sb.append(" [").append(d.getDataType().getName()).append("]");
            }
        }
        return sb.toString();
    }

    private String branchComment(Instruction ins) {
        Set<String> calls = new LinkedHashSet<>();
        Set<String> jumps = new LinkedHashSet<>();
        MemoryBlock block = currentProgram.getMemory().getBlock(ins.getAddress());
        String seg = null;
        long segBase = 0;
        if (block != null) {
            String bs = block.getStart().toString();
            String[] parts = bs.split(":");
            if (parts.length == 2) {
                try {
                    long startOff = Long.parseLong(parts[1], 16);
                    seg = parts[0];
                    segBase = block.getStart().getOffset() - startOff;
                }
                catch (NumberFormatException e) {
                    seg = null;
                }
            }
        }
        for (Reference r : ins.getReferencesFrom()) {
            if (r.getReferenceType().isCall()) {
                Function g = currentProgram.getFunctionManager().getFunctionAt(r.getToAddress());
                if (g == null) {
                    g = currentProgram.getFunctionManager()
                            .getFunctionContaining(r.getToAddress());
                }
                calls.add(g == null ? r.getToAddress().toString()
                        : g.getName() + " @ " + g.getEntryPoint());
            }
            else if (r.getReferenceType().isJump()) {
                Address t = r.getToAddress();
                String text = t.toString();
                if (seg != null && block.getStart().compareTo(t) <= 0
                        && block.getEnd().compareTo(t) >= 0) {
                    long off = t.getOffset() - segBase;
                    text = seg + ":" + String.format("%04x", off);
                }
                jumps.add(text);
            }
        }
        StringBuilder sb = new StringBuilder();
        if (!calls.isEmpty()) {
            sb.append("call -> ").append(String.join(", ", calls));
        }
        if (!jumps.isEmpty()) {
            if (sb.length() > 0) {
                sb.append(" | ");
            }
            sb.append("jump -> ").append(String.join(", ", jumps));
        }
        return sb.toString();
    }

    private String bytesHex(Instruction ins) {
        try {
            StringBuilder sb = new StringBuilder();
            for (byte b : ins.getBytes()) {
                sb.append(String.format("%02x", b & 0xff));
            }
            return sb.toString();
        }
        catch (Exception e) {
            return "??";
        }
    }

    private String key(Address a) {
        return a.toString().replace(':', '_');
    }

    private String safe(String s) {
        return s.replaceAll("[^A-Za-z0-9_.-]", "_");
    }
}