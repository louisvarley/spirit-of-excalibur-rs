// Remove USER_DEFINED DATA references added by the exploratory relocation scan.
// @category AI

import java.util.ArrayList;
import java.util.List;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.symbol.RefType;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;
import ghidra.program.model.symbol.SourceType;

public class ClearUserDataRefs extends GhidraScript {
    @Override
    protected void run() throws Exception {
        ReferenceIterator it = currentProgram.getReferenceManager()
                .getReferenceIterator(currentProgram.getMinAddress());
        List<Reference> kill = new ArrayList<>();
        while (it.hasNext()) {
            Reference r = it.next();
            if (r.getSource() == SourceType.USER_DEFINED && r.getReferenceType() == RefType.DATA) {
                kill.add(r);
            }
        }
        for (Reference r : kill) {
            removeReference(r);
        }
        println("REMOVED_USER_DATA_REFS=" + kill.size());
    }
}
