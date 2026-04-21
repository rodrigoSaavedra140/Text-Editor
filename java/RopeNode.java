public class RopeNode {

    RopeNode left;
    RopeNode right;
    String data;
    int weight;

    // constructor Hoja
    public RopeNode(String data) {
        this.left = null;
        this.right = null;
        this.data = data;
        this.weight = data.length();
    }

    // constructor interno
    public RopeNode(RopeNode left, RopeNode right) {
        this.left = left;
        this.right = right;
        this.data = "";
        this.weight = left.weight + right.weight;
    }
}
