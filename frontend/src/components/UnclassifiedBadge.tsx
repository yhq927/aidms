import { Badge } from "@/components/ui/badge";
import { UNCLASSIFIED_LABEL } from "@/lib/docTypes";

/** 未归类主体标示：当资料未关联任何主体时展示（entity_ids 为空） */
export function UnclassifiedBadge() {
  return (
    <Badge variant="warning" className="shrink-0">
      {UNCLASSIFIED_LABEL}
    </Badge>
  );
}
