import { FC } from "react";
import { Button } from "react-bootstrap";
import { FileUploader } from "react-drag-drop-files";
import Highlight from "react-highlight";
import { toast } from "react-toastify";

export const LoadPackedFile: FC = () => {
  const handleChange = (file: any) => {
    toast.promise(
      fetch("api/load-pack-file", {
        method: "put",
        body: file,
      }),
      {
        error: "upload failed",
        pending: "uploading",
        success: "uploaded successfully.",
      }
    );
  };

  return (
    <div className="ms-5">
      <div>You can import a new Rust toolchain using the pack command:</div>
      <Highlight className="text-start shell-session">
        crates-registry pack --pack_file /path/to/dst.tar --platforms
        x86_64-unknown-linux-gnu --rust-versions nightly-20-12-2022
      </Highlight>
      <div className="pb-3">
        Run crates-registry pack --help for more information
      </div>
      <FileUploader handleChange={handleChange} name="file" types={["tar"]}>
        <div className="ml-3 px-4 py-2 border border-info rounded d-flex flex-column justify-content-center">
          <Button className="d-block">Select file...</Button>
          <span>or drag and drop file here</span>
        </div>
      </FileUploader>
    </div>
  );
};
