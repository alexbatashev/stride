use std::sync::Arc;

use friday_agent::tools::calculator::CalculatorTool;
use friday_agent::{Tool, ToolRegistry};

use friday_agent::tools::subagent::SubAgentTool;
use uuid::Uuid;

use crate::tools::vfs::{
    ListFilesTool, ReadTextFileTool, VfsDocumentToMarkdownTool, VfsPresentationXmlToPptxTool,
};
use crate::vfs::Vfs;

pub const PRESENTATION_DRAFT_MODEL: &str = "presentation_draft";
pub const PRESENTATION_DRAFT_NAME: &str = "presentation_draft";

const SYSTEM_PROMPT: &str = r#"You are a PowerPoint draft presentation creation subagent.

Input from the main agent contains the target PPTX path and a complete slide-by-slide description for the whole deck. Create the whole presentation in one pass.

Process:
1. Read referenced workspace files only when needed.
2. Convert the entire deck to Friday presentation XML.
3. Call vfs_presentation_xml_to_pptx exactly once with the target path and complete XML.
4. Return a concise final message with the created path. Do not return the XML unless the tool fails.

Friday presentation XML format example:
<presentation size="wide">
  <slide layout="title">
    <title>Talk title</title>
    <subtitle>Authors, affiliation, venue</subtitle>
    <notes>30-second opening and audience framing.</notes>
  </slide>
  <slide layout="two-column">
    <title>Slide title</title>
    <columns>
      <left>
        <bullets>
          <item>Claim with quantitative detail</item>
          <item>Method or contribution</item>
        </bullets>
      </left>
      <right>
        <image src="/figures/result.png" x="7.0" y="1.5" w="5.5" h="3.5"/>
        <textbox x="7.0" y="5.2" w="5.5" h="0.4">Figure caption or takeaway.</textbox>
      </right>
    </columns>
    <notes>Presenter-only explanation, caveats, and transitions.</notes>
  </slide>
</presentation>

Rules:
- The root must be presentation. Use size="wide" unless the user asked for 4:3, then use size="standard".
- Wide slides are 13.333 by 7.5 inches and standard slides are 10 by 7.5 inches. Use calculator tool to compute all the sizes you need, do not rely on LLM computation.
- Include every requested slide in order inside the same XML document.
- Use layout="title" for title slides, layout="section" for divider/titular slides, layout="two-column" for claim-plus-evidence slides, and layout="title-content" for normal slides.
- Use workspace images with absolute paths in <image src="/path/to/image.png" .../>. Supported formats: PNG, JPEG, GIF, TIFF, BMP, EMF, WMF.
- Use <notes> on slides where presenter guidance matters. Keep notes factual and useful; do not duplicate slide text.
- Keep text concise enough to fit slides. Split dense material across slides instead of cramming.
- Sliedes must guide audience and convey messages the speaker can not by saying out loud. Avoid overloading slides with text.
- For scientific conference drafts, prefer this structure when no structure is given: title, problem, gap, method, experimental setup, main result, ablation/analysis, limitations, conclusion.
- Escape XML special characters.
- Do not invent source facts. If source material is missing, create a clearly labeled placeholder slide rather than asking the user.
- The target path must be an absolute workspace path ending in .pptx."#;

const TOOL_DESC: &str = r#"Presentation draft tool can create a PowerPoint slide deck initial version.

To make a good presentation, you must follow these guidelines:
- Specify key presentation parameters: slide size (wide or standard), tone of voice, etc.
- Describe the audience. If you are not sure, ask user first about presentation listeners.
- Come up with a structure of the presentation. Each slide must be about a single idea.
- Describe each slide in great detail. If user mentioned graphs, images, etc, mention paths in the slide description.
- Between the first slide and the rest of presentation there should be agenda slide.
- If the topic is too complicated, split slide deck into sections, describing a transition slides between each section.
- Specify output file location.

This prompt will be fed into a specialized model that will build each slide as described in PowerPoint format.
For user convenience, in the final response message provide a download link for the user.

Example prompt structure:

Output location: /~workspace/my_cool_presentation.pptx
Key requirements: wide slides, playful funny tone.
Target audience: scientific conference keynote.

SLIDES:
- Slide 1. Opening slide description.
- Slide 2. Agenda slide with detailed item description.
- Slide 3. Detailed slide content description.
- Slide 4. Detailed slide content description.
"#;

pub fn make_presentation_draft(vfs: Arc<Vfs>, workspace_id: Uuid, user_id: Uuid) -> SubAgentTool {
    let tool_registry = presentation_draft_tool_registry(vfs, workspace_id, user_id);

    SubAgentTool::new(
        PRESENTATION_DRAFT_NAME,
        "Presentation draft",
        TOOL_DESC,
        PRESENTATION_DRAFT_MODEL,
        SYSTEM_PROMPT,
        tool_registry,
    )
}

fn presentation_draft_tool_registry(
    vfs: Arc<Vfs>,
    workspace_id: Uuid,
    user_id: Uuid,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    let list = ListFilesTool {
        vfs: vfs.clone(),
        workspace_id,
    };
    registry.allow_tool(list.name());
    registry.register(list);

    let read = ReadTextFileTool {
        vfs: vfs.clone(),
        workspace_id,
    };
    registry.allow_tool(read.name());
    registry.register(read);

    let read_document = VfsDocumentToMarkdownTool {
        vfs: vfs.clone(),
        workspace_id,
    };
    registry.allow_tool(read_document.name());
    registry.register(read_document);

    let write_pptx = VfsPresentationXmlToPptxTool {
        vfs,
        workspace_id,
        owner: user_id,
        requires_confirmation: false,
    };
    registry.allow_tool(write_pptx.name());
    registry.register(write_pptx);

    let calculator = CalculatorTool {};
    registry.allow_tool(calculator.name());
    registry.register(calculator);

    registry
}
