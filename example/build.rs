use cmd_lib::run_cmd;

fn main() {
    run_cmd! {
      cd protocols;
      ./hack/update-generated-proto.sh;
    };
}
